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

/// Which step a video's progress belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum VideoStage {
    Downloading,
    Converting,
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
    /// yt-dlp's `-S` value: a preference, not a `-f` filter, so it still picks a stream when a
    /// fresh page extraction offers different heights. `height`, not `res`, which is the
    /// smaller side.
    pub(crate) fn sort(&self) -> Option<String> {
        let cap = self.max_height.map(|h| format!("height:{h}"));
        let short = self.chosen_height().is_some_and(|h| h <= H264_MAX_HEIGHT);
        match self.format {
            // The height comes before the codec, so H.264 only wins over VP9 or AV1 of the
            // same height, and before `acodec`, so a small muxed H.264+AAC stream never wins
            // over a taller video-only one. `lang` first keeps the original audio language.
            VideoFormat::AppleCompatible if short => {
                let height = cap.unwrap_or_else(|| "height".into());
                Some(format!("lang,{height},vcodec:h264,acodec:aac"))
            }
            _ => cap,
        }
    }

    /// The height the `height` sort lands on.
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

    fn sort(format: VideoFormat, max_height: Option<u32>, heights: &[u32]) -> Option<String> {
        StreamChoice { format, max_height, heights }.sort()
    }

    #[test]
    fn best_quality_sorts_only_by_the_height_cap() {
        assert_eq!(sort(VideoFormat::BestQuality, Some(720), HEIGHTS).as_deref(), Some("height:720"));
        assert_eq!(sort(VideoFormat::BestQuality, None, &[720]), None);
    }

    #[test]
    fn apple_devices_prefer_h264_and_aac_up_to_1080p() {
        assert_eq!(
            sort(VideoFormat::AppleCompatible, Some(1080), HEIGHTS).as_deref(),
            Some("lang,height:1080,vcodec:h264,acodec:aac")
        );
        assert_eq!(
            sort(VideoFormat::AppleCompatible, None, &[1080, 720]).as_deref(),
            Some("lang,height,vcodec:h264,acodec:aac")
        );
    }

    #[test]
    fn apple_devices_download_taller_videos_like_best_quality() {
        assert_eq!(sort(VideoFormat::AppleCompatible, Some(2160), HEIGHTS).as_deref(), Some("height:2160"));
        assert_eq!(sort(VideoFormat::AppleCompatible, None, HEIGHTS), None);
        assert_eq!(sort(VideoFormat::AppleCompatible, Some(1440), &[2160, 1440]).as_deref(), Some("height:1440"));
    }

    #[test]
    fn a_cap_between_heights_lands_on_the_shorter_stream() {
        assert_eq!(
            sort(VideoFormat::AppleCompatible, Some(1440), HEIGHTS).as_deref(),
            Some("lang,height:1440,vcodec:h264,acodec:aac")
        );
    }

    #[test]
    fn a_cap_below_every_stream_lands_on_the_shortest() {
        assert_eq!(
            sort(VideoFormat::AppleCompatible, Some(240), HEIGHTS).as_deref(),
            Some("lang,height:240,vcodec:h264,acodec:aac")
        );
        assert_eq!(sort(VideoFormat::AppleCompatible, Some(240), &[2160]).as_deref(), Some("height:240"));
    }

    #[test]
    fn unknown_heights_download_like_best_quality() {
        assert_eq!(sort(VideoFormat::AppleCompatible, None, &[]), None);
    }
}
