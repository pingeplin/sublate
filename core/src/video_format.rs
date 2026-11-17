use crate::ytdlp::StreamRequest;

/// YouTube offers H.264 up to this height; taller videos come only as VP9 or AV1.
const H264_MAX_HEIGHT: u32 = 1080;

/// What the downloaded video is made into.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum VideoFormat {
    /// The streams as downloaded, often AV1 or VP9 with Opus, which the Photos app rejects.
    BestQuality,
    /// What the Photos app accepts: H.264 or HEVC with AAC. Up to 1080p the download takes
    /// H.264 with AAC as offered; what is still not accepted is converted afterwards.
    AppleCompatible,
}

/// Which streams a download takes.
pub struct StreamChoice<'a> {
    pub format: VideoFormat,
    /// Caps the video at the tallest stream no taller than it, or the shortest one when every
    /// stream is taller; `None` takes the tallest.
    pub max_height: Option<u32>,
    /// Heights of the video streams on offer, largest first.
    pub heights: &'a [u32],
}

impl StreamChoice<'_> {
    pub fn request(&self) -> StreamRequest {
        let short = self.chosen_height().is_some_and(|h| h <= H264_MAX_HEIGHT);
        StreamRequest {
            max_height: self.max_height,
            prefer_h264_aac: self.format == VideoFormat::AppleCompatible && short,
        }
    }

    /// Whether the download is converted afterwards when its streams need it.
    pub fn converts(&self) -> bool {
        self.format == VideoFormat::AppleCompatible
    }

    /// The height the download lands on.
    fn chosen_height(&self) -> Option<u32> {
        let Some(cap) = self.max_height else {
            return self.heights.iter().copied().max();
        };
        let fitting = self.heights.iter().copied().filter(|h| *h <= cap).max();
        fitting.or_else(|| self.heights.iter().copied().min())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEIGHTS: &[u32] = &[2160, 1080, 720, 360];

    fn prefers_h264(format: VideoFormat, max_height: Option<u32>, heights: &[u32]) -> bool {
        StreamChoice { format, max_height, heights }.request().prefer_h264_aac
    }

    #[test]
    fn the_request_keeps_the_height_cap() {
        let choice = StreamChoice { format: VideoFormat::AppleCompatible, max_height: Some(720), heights: HEIGHTS };

        assert_eq!(choice.request(), StreamRequest { max_height: Some(720), prefer_h264_aac: true });
    }

    #[test]
    fn only_apple_devices_convert_after_the_download() {
        let converts = |format| StreamChoice { format, max_height: None, heights: HEIGHTS }.converts();

        assert!(converts(VideoFormat::AppleCompatible));
        assert!(!converts(VideoFormat::BestQuality));
    }

    #[test]
    fn best_quality_never_prefers_h264() {
        assert!(!prefers_h264(VideoFormat::BestQuality, Some(720), HEIGHTS));
        assert!(!prefers_h264(VideoFormat::BestQuality, None, &[720]));
    }

    #[test]
    fn apple_devices_prefer_h264_up_to_1080p() {
        assert!(prefers_h264(VideoFormat::AppleCompatible, Some(1080), HEIGHTS));
        assert!(prefers_h264(VideoFormat::AppleCompatible, None, &[1080, 720]));
    }

    #[test]
    fn apple_devices_download_taller_videos_like_best_quality() {
        assert!(!prefers_h264(VideoFormat::AppleCompatible, Some(2160), HEIGHTS));
        assert!(!prefers_h264(VideoFormat::AppleCompatible, None, HEIGHTS));
        assert!(!prefers_h264(VideoFormat::AppleCompatible, Some(1440), &[2160, 1440]));
    }

    #[test]
    fn a_cap_between_heights_lands_on_the_shorter_stream() {
        assert!(prefers_h264(VideoFormat::AppleCompatible, Some(1440), HEIGHTS));
    }

    #[test]
    fn a_cap_below_every_stream_lands_on_the_shortest() {
        assert!(prefers_h264(VideoFormat::AppleCompatible, Some(240), HEIGHTS));
        assert!(!prefers_h264(VideoFormat::AppleCompatible, Some(240), &[2160]));
    }

    #[test]
    fn unknown_heights_download_like_best_quality() {
        assert!(!prefers_h264(VideoFormat::AppleCompatible, None, &[]));
    }
}
