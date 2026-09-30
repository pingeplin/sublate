use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cue {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

pub fn parse_srt(input: &str) -> AppResult<Vec<Cue>> {
    let normalized = input.trim_start_matches('\u{feff}').replace("\r\n", "\n");
    let mut cues = Vec::new();
    let mut lines = normalized.lines();
    while let Some(line) = lines.next() {
        let Some((start, end)) = line.split_once("-->") else {
            continue;
        };
        let text = lines
            .by_ref()
            .take_while(|l| !l.is_empty())
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        cues.push(Cue {
            start_ms: parse_timestamp(start)?,
            end_ms: parse_timestamp(end)?,
            text,
        });
    }
    Ok(cues)
}

pub fn to_srt(cues: &[Cue]) -> String {
    cues.iter()
        .enumerate()
        .map(|(i, cue)| {
            format!(
                "{}\n{} --> {}\n{}\n",
                i + 1,
                format_timestamp(cue.start_ms),
                format_timestamp(cue.end_ms),
                cue.text
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// YouTube auto-captions scroll: each cue repeats the previous cue's last line, and
/// 10ms "flash" cues hold only the carried-over line. Keep only the new text.
pub fn collapse_rolling(cues: Vec<Cue>) -> Vec<Cue> {
    let mut out = Vec::with_capacity(cues.len());
    let mut previous_line: Option<String> = None;
    for cue in cues {
        let mut lines: Vec<&str> = cue.text.lines().collect();
        if lines.first().copied() == previous_line.as_deref() {
            lines.remove(0);
        }
        let Some(last) = lines.last() else {
            continue;
        };
        previous_line = Some(last.to_string());
        out.push(Cue {
            text: lines.join("\n"),
            ..cue
        });
    }
    out
}

/// Word order can move a cue's meaning into its neighbour (e.g. Korean verb-final
/// fragments), leaving an empty translation. Fold empty cues into the previous cue's
/// time span, or into the next cue when there is no previous one.
pub fn absorb_empty_cues(cues: Vec<Cue>) -> Vec<Cue> {
    let mut out: Vec<Cue> = Vec::with_capacity(cues.len());
    let mut pending_start: Option<u64> = None;
    for cue in cues {
        if cue.text.trim().is_empty() {
            match out.last_mut() {
                Some(prev) => prev.end_ms = prev.end_ms.max(cue.end_ms),
                None => pending_start = Some(pending_start.unwrap_or(cue.start_ms)),
            }
            continue;
        }
        let start_ms = pending_start.take().unwrap_or(cue.start_ms);
        out.push(Cue { start_ms, ..cue });
    }
    out
}

fn parse_timestamp(raw: &str) -> AppResult<u64> {
    let invalid = || AppError::Subtitle(format!("bad timestamp '{}'", raw.trim()));
    let token = raw.split_whitespace().next().ok_or_else(invalid)?;
    let (clock, millis) = token.split_once([',', '.']).ok_or_else(invalid)?;
    let parts: Vec<u64> = clock
        .split(':')
        .map(|p| p.parse().map_err(|_| invalid()))
        .collect::<AppResult<_>>()?;
    let [h, m, s] = parts[..] else {
        return Err(invalid());
    };
    let ms: u64 = millis.parse().map_err(|_| invalid())?;
    Ok(((h * 60 + m) * 60 + s) * 1000 + ms)
}

fn format_timestamp(ms: u64) -> String {
    format!(
        "{:02}:{:02}:{:02},{:03}",
        ms / 3_600_000,
        ms / 60_000 % 60,
        ms / 1000 % 60,
        ms % 1000
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_timestamp_with_comma_or_dot() {
        assert_eq!(parse_timestamp("01:02:03,004").unwrap(), 3_723_004);
        assert_eq!(parse_timestamp(" 00:00:02.869 ").unwrap(), 2_869);
    }

    #[test]
    fn rejects_malformed_timestamp() {
        assert!(parse_timestamp("00:02,000").is_err());
        assert!(parse_timestamp("aa:bb:cc,ddd").is_err());
    }

    #[test]
    fn formats_timestamp() {
        assert_eq!(format_timestamp(3_723_004), "01:02:03,004");
    }
}
