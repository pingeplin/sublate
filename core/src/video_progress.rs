//! The steps of one video job and how far each has come, as the app shows them.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use async_trait::async_trait;

use crate::error::AppResult;

/// One step of a video job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum VideoStep {
    /// The video-only stream of a download that merges two streams.
    Video,
    /// The audio-only stream of a download that merges two streams.
    Audio,
    /// A stream that holds both video and audio.
    VideoWithAudio,
    /// yt-dlp merges the streams, remuxes, and embeds the cover art; it reports no percent.
    Finishing,
    /// The conversion for Apple devices, which is not needed for every video.
    Converting,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct ByteProgress {
    pub done: u64,
    pub total: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, uniffi::Enum)]
pub enum StepState {
    Waiting,
    Running { percent: Option<f32>, bytes: Option<ByteProgress> },
    Done,
    Skipped,
    /// Keeps the last values the step reported.
    Failed { percent: Option<f32>, bytes: Option<ByteProgress> },
}

#[derive(Debug, Clone, Copy, PartialEq, uniffi::Record)]
pub struct VideoStepProgress {
    pub step: VideoStep,
    pub state: StepState,
}

/// A full snapshot of the job, so a report that is dropped on the way loses nothing.
/// `steps` is empty until yt-dlp names the streams it downloads.
#[derive(Debug, Clone, Default, PartialEq, uniffi::Record)]
pub struct VideoProgress {
    pub steps: Vec<VideoStepProgress>,
}

/// What a yt-dlp video download reports.
#[derive(Debug, Clone, PartialEq)]
pub enum DownloadEvent {
    /// The formats yt-dlp downloads, in the order it downloads them.
    Streams { format_ids: Vec<String> },
    /// A progress line of one stream; `finished` on its last line.
    Stream {
        format_id: String,
        finished: bool,
        percent: Option<f32>,
        bytes: Option<ByteProgress>,
    },
}

/// Turns the reports of one video job into snapshots, and hands each changed snapshot to
/// `on_progress`.
pub struct VideoTracker<F: Fn(VideoProgress)> {
    converts: bool,
    format_ids: Vec<String>,
    progress: VideoProgress,
    on_progress: F,
}

impl<F: Fn(VideoProgress)> VideoTracker<F> {
    /// `converts` adds the Converting step to every list.
    pub fn new(converts: bool, on_progress: F) -> Self {
        Self {
            converts,
            format_ids: Vec::new(),
            progress: VideoProgress::default(),
            on_progress,
        }
    }

    pub fn download(&mut self, event: DownloadEvent) {
        match event {
            DownloadEvent::Streams { format_ids } => {
                let streams = stream_steps(format_ids.len());
                let rest = [VideoStep::Finishing].into_iter().chain(self.converts.then_some(VideoStep::Converting));
                let steps = streams.into_iter().chain(rest).map(|step| VideoStepProgress { step, state: StepState::Waiting });
                self.format_ids = format_ids;
                self.update(|progress| progress.steps = steps.collect());
            }
            DownloadEvent::Stream { format_id, finished, percent, bytes } => {
                let Some(index) = self.format_ids.iter().position(|id| *id == format_id) else {
                    return;
                };
                let last = index + 1 == self.format_ids.len();
                self.update(|progress| {
                    progress.steps[..index].iter_mut().for_each(|s| s.state = StepState::Done);
                    progress.steps[index].state = if finished {
                        StepState::Done
                    } else {
                        StepState::Running { percent, bytes }
                    };
                    if finished && last {
                        progress.set(VideoStep::Finishing, StepState::Running { percent: None, bytes: None });
                    }
                });
            }
        }
    }

    /// yt-dlp exited with success.
    pub fn download_finished(&mut self) {
        self.update(|progress| {
            progress
                .steps
                .iter_mut()
                .filter(|s| s.step != VideoStep::Converting)
                .for_each(|s| s.state = StepState::Done);
        });
    }

    pub fn converting(&mut self, percent: Option<f32>) {
        self.update(|progress| progress.set(VideoStep::Converting, StepState::Running { percent, bytes: None }));
    }

    /// `converted` is `false` when the streams needed no conversion.
    pub fn conversion_finished(&mut self, converted: bool) {
        let state = if converted { StepState::Done } else { StepState::Skipped };
        self.update(|progress| progress.set(VideoStep::Converting, state));
    }

    /// Marks the running step, else the first unfinished one, as failed.
    pub fn fail(&mut self) {
        self.update(|progress| {
            let running = progress.steps.iter_mut().find_map(|s| match s.state {
                StepState::Running { percent, bytes } => Some((s, percent, bytes)),
                _ => None,
            });
            if let Some((step, percent, bytes)) = running {
                step.state = StepState::Failed { percent, bytes };
            } else if let Some(step) =
                progress.steps.iter_mut().find(|s| !matches!(s.state, StepState::Done | StepState::Skipped))
            {
                step.state = StepState::Failed { percent: None, bytes: None };
            }
        });
    }

    fn update(&mut self, change: impl FnOnce(&mut VideoProgress)) {
        let before = self.progress.clone();
        change(&mut self.progress);
        if self.progress != before {
            (self.on_progress)(self.progress.clone());
        }
    }
}

/// The two parts of a video job that run outside the core: the download and the conversion.
#[async_trait]
pub trait VideoWork: Sync {
    async fn download(&self, on_event: &(dyn Fn(DownloadEvent) + Sync)) -> AppResult<PathBuf>;
    /// Tells whether the file needed a conversion.
    async fn convert(&self, video: &Path, on_percent: &(dyn Fn(f32) + Sync)) -> AppResult<bool>;
}

/// Downloads and then, when `converts`, converts the file, reporting each step to
/// `on_progress`. On an error the step that was running is marked failed.
pub async fn track_video(
    work: &dyn VideoWork,
    converts: bool,
    on_progress: impl Fn(VideoProgress) + Send,
) -> AppResult<PathBuf> {
    let tracker = Mutex::new(VideoTracker::new(converts, on_progress));
    let track = |change: &dyn Fn(&mut VideoTracker<_>)| change(&mut tracker.lock().expect("video tracker lock"));
    let result = async {
        let path = work.download(&|event| track(&|t| t.download(event.clone()))).await?;
        track(&|t| t.download_finished());
        if converts {
            track(&|t| t.converting(None));
            let converted = work.convert(&path, &|percent| track(&|t| t.converting(Some(percent)))).await?;
            track(&|t| t.conversion_finished(converted));
        }
        Ok(path)
    }
    .await;
    if result.is_err() {
        track(&|t| t.fail());
    }
    result
}

impl VideoProgress {
    fn set(&mut self, step: VideoStep, state: StepState) {
        if let Some(entry) = self.steps.iter_mut().find(|s| s.step == step) {
            entry.state = state;
        }
    }
}

/// yt-dlp downloads the video stream before the audio stream.
fn stream_steps(count: usize) -> Vec<VideoStep> {
    match count {
        2 => vec![VideoStep::Video, VideoStep::Audio],
        _ => vec![VideoStep::VideoWithAudio; count],
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;
    use crate::error::AppError;
    use StepState::*;
    use VideoStep::*;

    /// Runs `actions` on a tracker and hands back every snapshot it sent.
    fn snapshots(converts: bool, actions: impl FnOnce(&mut VideoTracker<&dyn Fn(VideoProgress)>)) -> Vec<VideoProgress> {
        let sent = RefCell::new(Vec::new());
        let record = |p: VideoProgress| sent.borrow_mut().push(p);
        let mut tracker = VideoTracker::new(converts, &record as &dyn Fn(VideoProgress));
        actions(&mut tracker);
        drop(tracker);
        sent.into_inner()
    }

    fn last(converts: bool, actions: impl FnOnce(&mut VideoTracker<&dyn Fn(VideoProgress)>)) -> Vec<(VideoStep, StepState)> {
        let all = snapshots(converts, actions);
        all.last().map(states).unwrap_or_default()
    }

    fn states(progress: &VideoProgress) -> Vec<(VideoStep, StepState)> {
        progress.steps.iter().map(|s| (s.step, s.state)).collect()
    }

    fn streams(ids: &[&str]) -> DownloadEvent {
        DownloadEvent::Streams { format_ids: ids.iter().map(|id| id.to_string()).collect() }
    }

    fn line(id: &str, finished: bool, percent: Option<f32>, bytes: Option<ByteProgress>) -> DownloadEvent {
        DownloadEvent::Stream { format_id: id.into(), finished, percent, bytes }
    }

    fn downloading(id: &str, percent: f32) -> DownloadEvent {
        line(id, false, Some(percent), None)
    }

    fn finished(id: &str) -> DownloadEvent {
        line(id, true, Some(100.0), None)
    }

    const BYTES: ByteProgress = ByteProgress { done: 1_100_000, total: Some(3_600_000) };

    #[test]
    fn two_streams_make_a_video_an_audio_and_a_finishing_step() {
        let steps = last(false, |t| t.download(streams(&["395", "251"])));

        assert_eq!(steps, [(Video, Waiting), (Audio, Waiting), (Finishing, Waiting)]);
    }

    #[test]
    fn apple_devices_add_the_converting_step_from_the_start() {
        let steps = last(true, |t| t.download(streams(&["395", "251"])));

        assert_eq!(steps, [(Video, Waiting), (Audio, Waiting), (Finishing, Waiting), (Converting, Waiting)]);
    }

    #[test]
    fn one_stream_makes_a_video_with_audio_step() {
        let steps = last(false, |t| t.download(streams(&["18"])));

        assert_eq!(steps, [(VideoWithAudio, Waiting), (Finishing, Waiting)]);
    }

    #[test]
    fn three_streams_make_three_video_with_audio_steps() {
        let steps = last(false, |t| t.download(streams(&["a", "b", "c"])));

        assert_eq!(steps, [(VideoWithAudio, Waiting), (VideoWithAudio, Waiting), (VideoWithAudio, Waiting), (Finishing, Waiting)]);
    }

    #[test]
    fn a_line_of_the_audio_stream_finishes_the_video_stream() {
        let steps = last(false, |t| {
            t.download(streams(&["395", "251"]));
            t.download(line("251", false, Some(30.0), Some(BYTES)));
        });

        let audio = Running { percent: Some(30.0), bytes: Some(BYTES) };
        assert_eq!(steps, [(Video, Done), (Audio, audio), (Finishing, Waiting)]);
    }

    #[test]
    fn the_end_of_the_last_stream_starts_finishing() {
        let steps = last(false, |t| {
            t.download(streams(&["395", "251"]));
            t.download(downloading("251", 50.0));
            t.download(finished("251"));
        });

        let finishing = Running { percent: None, bytes: None };
        assert_eq!(steps, [(Video, Done), (Audio, Done), (Finishing, finishing)]);
    }

    #[test]
    fn the_end_of_an_earlier_stream_does_not_start_finishing() {
        let steps = last(false, |t| {
            t.download(streams(&["395", "251"]));
            t.download(finished("395"));
        });

        assert_eq!(steps, [(Video, Done), (Audio, Waiting), (Finishing, Waiting)]);
    }

    #[test]
    fn a_file_already_on_disk_goes_straight_to_finishing() {
        let all = snapshots(false, |t| {
            t.download(streams(&["18"]));
            t.download(line("18", true, Some(100.0), None));
            t.download_finished();
        });

        let finishing = Running { percent: None, bytes: None };
        assert_eq!(states(&all[1]), [(VideoWithAudio, Done), (Finishing, finishing)]);
        assert_eq!(states(all.last().unwrap()), [(VideoWithAudio, Done), (Finishing, Done)]);
    }

    #[test]
    fn a_success_without_download_lines_finishes_every_step() {
        let steps = last(false, |t| {
            t.download(streams(&["395", "251"]));
            t.download_finished();
        });

        assert_eq!(steps, [(Video, Done), (Audio, Done), (Finishing, Done)]);
    }

    #[test]
    fn a_second_extraction_starts_the_list_again() {
        let steps = last(false, |t| {
            t.download(streams(&["395", "251"]));
            t.download(downloading("251", 40.0));
            t.download(streams(&["395", "251"]));
        });

        assert_eq!(steps, [(Video, Waiting), (Audio, Waiting), (Finishing, Waiting)]);
    }

    #[test]
    fn after_a_second_extraction_the_first_stream_runs_again() {
        let steps = last(false, |t| {
            t.download(streams(&["395", "251"]));
            t.download(downloading("251", 40.0));
            t.download(streams(&["395", "251"]));
            t.download(downloading("395", 5.0));
        });

        let video = Running { percent: Some(5.0), bytes: None };
        assert_eq!(steps, [(Video, video), (Audio, Waiting), (Finishing, Waiting)]);
    }

    #[test]
    fn a_line_of_an_unknown_format_changes_nothing() {
        let all = snapshots(false, |t| {
            t.download(streams(&["395", "251"]));
            t.download(downloading("999", 40.0));
        });

        assert_eq!(all.len(), 1);
    }

    #[test]
    fn a_conversion_runs_after_the_download_and_ends_done() {
        let all = snapshots(true, |t| {
            t.download(streams(&["18"]));
            t.download_finished();
            t.converting(None);
            t.converting(Some(40.0));
            t.conversion_finished(true);
        });

        let at = |i: usize| states(&all[i]);
        assert_eq!(at(2), [(VideoWithAudio, Done), (Finishing, Done), (Converting, Running { percent: None, bytes: None })]);
        assert_eq!(at(3), [(VideoWithAudio, Done), (Finishing, Done), (Converting, Running { percent: Some(40.0), bytes: None })]);
        assert_eq!(at(4), [(VideoWithAudio, Done), (Finishing, Done), (Converting, Done)]);
    }

    #[test]
    fn a_conversion_that_is_not_needed_is_skipped() {
        let steps = last(true, |t| {
            t.download(streams(&["18"]));
            t.download_finished();
            t.converting(None);
            t.conversion_finished(false);
        });

        assert_eq!(steps, [(VideoWithAudio, Done), (Finishing, Done), (Converting, Skipped)]);
    }

    #[test]
    fn a_failure_keeps_the_last_values_of_the_running_step() {
        let steps = last(false, |t| {
            t.download(streams(&["395", "251"]));
            t.download(line("251", false, Some(30.0), Some(BYTES)));
            t.fail();
        });

        let audio = Failed { percent: Some(30.0), bytes: Some(BYTES) };
        assert_eq!(steps, [(Video, Done), (Audio, audio), (Finishing, Waiting)]);
    }

    #[test]
    fn a_failure_with_nothing_running_fails_the_first_unfinished_step() {
        let steps = last(false, |t| {
            t.download(streams(&["395", "251"]));
            t.fail();
        });

        let failed = Failed { percent: None, bytes: None };
        assert_eq!(steps, [(Video, failed), (Audio, Waiting), (Finishing, Waiting)]);
    }

    /// Reports `events`, then fails or hands back a path; converts the same way.
    struct FakeWork {
        events: Vec<DownloadEvent>,
        download_fails: bool,
        conversion: Result<(Vec<f32>, bool), ()>,
    }

    #[async_trait]
    impl VideoWork for FakeWork {
        async fn download(&self, on_event: &(dyn Fn(DownloadEvent) + Sync)) -> AppResult<PathBuf> {
            self.events.iter().cloned().for_each(on_event);
            if self.download_fails {
                return Err(AppError::YtDlp("network".into()));
            }
            Ok("/out/video.mp4".into())
        }

        async fn convert(&self, _: &Path, on_percent: &(dyn Fn(f32) + Sync)) -> AppResult<bool> {
            let (percents, converted) = self.conversion.clone().map_err(|_| AppError::Convert("no encoder".into()))?;
            percents.into_iter().for_each(on_percent);
            Ok(converted)
        }
    }

    fn two_streams_downloaded() -> Vec<DownloadEvent> {
        vec![streams(&["395", "251"]), finished("395"), finished("251")]
    }

    fn run_job(work: FakeWork, converts: bool) -> (AppResult<PathBuf>, Vec<Vec<(VideoStep, StepState)>>) {
        let sent = Mutex::new(Vec::new());
        let runtime = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let result = runtime.block_on(track_video(&work, converts, |p| sent.lock().unwrap().push(states(&p))));
        (result, sent.into_inner().unwrap())
    }

    #[test]
    fn a_job_that_converts_ends_with_every_step_done() {
        let work = FakeWork { events: two_streams_downloaded(), download_fails: false, conversion: Ok((vec![40.0], true)) };

        let (result, sent) = run_job(work, true);

        assert_eq!(result.unwrap(), Path::new("/out/video.mp4"));
        let after_download = |state| vec![(Video, Done), (Audio, Done), (Finishing, Done), (Converting, state)];
        let downloaded = sent.iter().position(|s| s[2] == (Finishing, Done)).unwrap();
        assert_eq!(
            sent[downloaded..],
            [
                after_download(Waiting),
                after_download(Running { percent: None, bytes: None }),
                after_download(Running { percent: Some(40.0), bytes: None }),
                after_download(Done),
            ]
        );
    }

    #[test]
    fn a_job_whose_streams_are_accepted_skips_the_conversion() {
        let work = FakeWork { events: two_streams_downloaded(), download_fails: false, conversion: Ok((vec![], false)) };

        let (_, sent) = run_job(work, true);

        assert_eq!(sent.last().unwrap(), &[(Video, Done), (Audio, Done), (Finishing, Done), (Converting, Skipped)]);
    }

    #[test]
    fn a_job_that_does_not_convert_never_runs_the_conversion() {
        let work = FakeWork { events: two_streams_downloaded(), download_fails: false, conversion: Err(()) };

        let (result, sent) = run_job(work, false);

        assert!(result.is_ok());
        assert_eq!(sent.last().unwrap(), &[(Video, Done), (Audio, Done), (Finishing, Done)]);
    }

    #[test]
    fn a_failed_download_marks_the_running_stream_failed_and_returns_the_error() {
        let events = vec![streams(&["395", "251"]), finished("395"), line("251", false, Some(30.0), Some(BYTES))];
        let work = FakeWork { events, download_fails: true, conversion: Ok((vec![], true)) };

        let (result, sent) = run_job(work, true);

        assert_eq!(result.unwrap_err().to_string(), AppError::YtDlp("network".into()).to_string());
        let audio = Failed { percent: Some(30.0), bytes: Some(BYTES) };
        assert_eq!(sent.last().unwrap(), &[(Video, Done), (Audio, audio), (Finishing, Waiting), (Converting, Waiting)]);
    }

    #[test]
    fn a_failed_conversion_marks_the_conversion_failed() {
        let work = FakeWork { events: two_streams_downloaded(), download_fails: false, conversion: Err(()) };

        let (result, sent) = run_job(work, true);

        assert!(result.is_err());
        let converting = Failed { percent: None, bytes: None };
        assert_eq!(sent.last().unwrap(), &[(Video, Done), (Audio, Done), (Finishing, Done), (Converting, converting)]);
    }

    #[test]
    fn a_failure_before_the_streams_are_known_sends_nothing() {
        let all = snapshots(false, |t| t.fail());

        assert!(all.is_empty());
    }
}
