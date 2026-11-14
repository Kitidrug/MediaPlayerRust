pub fn truncate_ellipsis(text: &str, max_chars: usize) -> String {
    let char_count = text.chars().count();
    if char_count <= max_chars {
        text.to_string()
    } else {
        let truncated: String = text.chars().take(max_chars.saturating_sub(2)).collect();
        format!("{}…", truncated.trim_end())
    }
}

pub fn clean_title(raw: &str) -> String {
    let mut title = raw.trim().to_string();

    // 1. Strip folder paths or URL segments if a full path was passed
    if let Some(pos) = title.rfind(|c| c == '/' || c == '\\') {
        title = title[pos + 1..].to_string();
    }

    // 2. Remove file extensions (.mkv, .mp4, .avi, etc.)
    if let Some(pos) = title.rfind('.') {
        let ext = &title[pos + 1..];
        if ["mkv", "mp4", "avi", "mov", "webm", "ts", "m4v", "flv", "wmv", "mp3", "wav", "flac", "ogg"]
            .contains(&ext.to_lowercase().as_str())
        {
            title.truncate(pos);
        }
    }

    // 3. Remove square brackets [...] and parentheses (...) containing release / codec / group tags
    let mut cleaned = String::new();
    let mut chars = title.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '[' {
            let mut inside = String::new();
            while let Some(&inner) = chars.peek() {
                chars.next();
                if inner == ']' {
                    break;
                }
                inside.push(inner);
            }
            let lower = inside.to_lowercase();
            let is_crc32 = inside.len() == 8 && inside.chars().all(|ch| ch.is_ascii_hexdigit());
            let is_junk = is_crc32
                || lower.contains("1080")
                || lower.contains("720")
                || lower.contains("480")
                || lower.contains("4k")
                || lower.contains("2160")
                || lower.contains("web")
                || lower.contains("dl")
                || lower.contains("rip")
                || lower.contains("bluray")
                || lower.contains("bd")
                || lower.contains("hdtv")
                || lower.contains("remux")
                || lower.contains("nf")
                || lower.contains("cr")
                || lower.contains("x264")
                || lower.contains("x265")
                || lower.contains("hevc")
                || lower.contains("avc")
                || lower.contains("av1")
                || lower.contains("10bit")
                || lower.contains("aac")
                || lower.contains("ac3")
                || lower.contains("eac3")
                || lower.contains("dts")
                || lower.contains("flac")
                || lower.contains("dub")
                || lower.contains("sub")
                || lower.contains("raw")
                || lower.contains(".com")
                || lower.contains(".net")
                || lower.contains(".org")
                || lower.contains(".ru")
                || lower.contains("save")
                || lower.contains("anidub")
                || lower.contains("anilibria")
                || lower.contains("animevost")
                || lower.contains("lostfilm")
                || lower.contains("shiza")
                || lower.contains("dual")
                || lower.contains("multi");

            // Strip any bracket at the start of filename (e.g. [SubsPlease] Title)
            // or at the end of filename, or if it contains junk keywords
            let is_edge = cleaned.trim().is_empty() || chars.peek().is_none();
            if !is_junk && !is_edge {
                cleaned.push('[');
                cleaned.push_str(&inside);
                cleaned.push(']');
            }
        } else if c == '(' {
            let mut inside = String::new();
            while let Some(&inner) = chars.peek() {
                chars.next();
                if inner == ')' {
                    break;
                }
                inside.push(inner);
            }
            let lower = inside.to_lowercase();
            let is_junk = lower.contains("1080p")
                || lower.contains("720p")
                || lower.contains("480p")
                || lower.contains("4k")
                || lower.contains("web-dl")
                || lower.contains("webrip")
                || lower.contains("bdrip")
                || lower.contains("x264")
                || lower.contains("x265")
                || lower.contains("hevc")
                || lower.contains("avc")
                || lower.contains("aac")
                || lower.contains("dub")
                || lower.contains(".com");

            if !is_junk {
                cleaned.push('(');
                cleaned.push_str(&inside);
                cleaned.push(')');
            }
        } else {
            cleaned.push(c);
        }
    }
    title = cleaned;

    // 4. Dot & Underscore normalization for scene releases
    if title.contains('.') && !title.contains(' ') {
        title = title.replace('.', " ");
    }
    title = title.replace('_', " ");

    // 5. Remove trailing scene keywords if they appear as standalone words
    let scene_keywords = [
        "1080p", "720p", "480p", "2160p", "4k", "web-dl", "webrip", "bdrip", "bluray", "hdtv",
        "x264", "x265", "hevc", "avc", "10bit", "aac", "ac3", "dts", "lostfilm", "anidub",
        "remux", "megusta", "proper", "repack",
    ];

    let mut words: Vec<&str> = title.split_whitespace().collect();
    while let Some(last) = words.last() {
        let last_lower = last.to_lowercase();
        let clean_last = last_lower.trim_matches(|c: char| !c.is_alphanumeric());
        if scene_keywords.contains(&clean_last) {
            words.pop();
        } else {
            break;
        }
    }
    title = words.join(" ");

    // 6. Strip trailing/leading hyphens, dots, underscores and colons
    let result = title
        .trim()
        .trim_matches(|c: char| c == '-' || c == '•' || c == ':' || c == '_' || c == '.' || c == '~')
        .trim()
        .to_string();

    if result.is_empty() {
        raw.trim().to_string()
    } else {
        result
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioTrack {
    pub id: i32,
    pub language: String,
    pub language_name: String,
    pub codec: String,
    pub title: String,
    pub channels: Option<i32>,
    pub sample_rate: Option<i32>,
    pub bitrate: Option<u32>,
}

impl AudioTrack {
    /// Short, clean voiceover name for the top dropdown button (e.g. "AniDUB", "LostFilm", "Дубляж")
    pub fn short_name(&self) -> String {
        let base: String = if !self.title.is_empty() {
            self.title.clone()
        } else if !self.language_name.is_empty() {
            self.language_name.clone()
        } else if !self.language.is_empty() {
            match self.language.to_lowercase().as_str() {
                "rus" | "ru" => "Русская".to_string(),
                "eng" | "en" => "English".to_string(),
                "jpn" | "ja" => "Japanese".to_string(),
                "ger" | "de" => "Deutsch".to_string(),
                "fra" | "fr" => "Français".to_string(),
                other => other.to_string(),
            }
        } else {
            "По умолчанию".to_string()
        };
        truncate_ellipsis(&base, 20)
    }

    /// Full descriptive title for detailed dropdown list items
    pub fn display_name(&self) -> String {
        let main_title = if !self.title.is_empty() {
            self.title.clone()
        } else if !self.language_name.is_empty() {
            self.language_name.clone()
        } else if !self.language.is_empty() {
            match self.language.to_lowercase().as_str() {
                "rus" | "ru" => "Русская озвучка".to_string(),
                "eng" | "en" => "English Audio".to_string(),
                "jpn" | "ja" => "Japanese Audio".to_string(),
                "ger" | "de" => "Deutsch Audio".to_string(),
                "fra" | "fr" => "Français Audio".to_string(),
                "ita" | "it" => "Italiano Audio".to_string(),
                "spa" | "es" => "Español Audio".to_string(),
                "kor" | "ko" => "Korean Audio".to_string(),
                "zho" | "chi" | "zh" => "Chinese Audio".to_string(),
                other => format!("Озвучка [{}]", other.to_uppercase()),
            }
        } else {
            format!("Озвучка #{}", self.id + 1)
        };

        let mut badges = Vec::new();

        if !self.language.is_empty()
            && !main_title.to_lowercase().contains(&self.language.to_lowercase())
        {
            badges.push(self.language.to_uppercase());
        }

        if let Some(ch) = self.channels {
            match ch {
                1 => badges.push("Mono".to_string()),
                2 => badges.push("Stereo".to_string()),
                6 => badges.push("5.1".to_string()),
                8 => badges.push("7.1".to_string()),
                n => badges.push(format!("{}ch", n)),
            }
        }

        if !self.codec.is_empty() {
            let codec_clean = if self.codec.contains("AC-3") || self.codec.contains("AC3") {
                "AC3"
            } else if self.codec.contains("E-AC-3") || self.codec.contains("EAC3") {
                "E-AC3"
            } else if self.codec.contains("AAC") {
                "AAC"
            } else if self.codec.contains("DTS-HD") {
                "DTS-HD"
            } else if self.codec.contains("DTS") {
                "DTS"
            } else if self.codec.contains("FLAC") {
                "FLAC"
            } else if self.codec.contains("Opus") {
                "Opus"
            } else if self.codec.contains("Vorbis") {
                "Vorbis"
            } else {
                &self.codec
            };
            badges.push(codec_clean.to_string());
        }

        if let Some(br) = self.bitrate {
            if br > 0 {
                badges.push(format!("{}k", br / 1000));
            }
        }

        if badges.is_empty() {
            format!("{}: {}", self.id + 1, main_title)
        } else {
            format!("{}: {} ({})", self.id + 1, main_title, badges.join(" • "))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubtitleTrack {
    pub id: i32, // -1 indicates subtitles disabled
    pub language: String,
    pub language_name: String,
    pub title: String,
}

impl SubtitleTrack {
    pub fn short_name(&self) -> String {
        if self.id < 0 {
            return "Откл.".to_string();
        }
        let base = if !self.title.is_empty() {
            &self.title
        } else if !self.language_name.is_empty() {
            &self.language_name
        } else if !self.language.is_empty() {
            &self.language
        } else {
            "Вкл."
        };
        truncate_ellipsis(base, 14)
    }

    pub fn display_name(&self) -> String {
        if self.id < 0 {
            return "Отключены (Off)".to_string();
        }

        let main_title = if !self.title.is_empty() {
            self.title.clone()
        } else if !self.language_name.is_empty() {
            self.language_name.clone()
        } else if !self.language.is_empty() {
            match self.language.to_lowercase().as_str() {
                "rus" | "ru" => "Русские субтитры".to_string(),
                "eng" | "en" => "English Subtitles".to_string(),
                other => format!("Субтитры [{}]", other.to_uppercase()),
            }
        } else {
            format!("Субтитры #{}", self.id + 1)
        };

        if !self.language.is_empty()
            && !main_title.to_lowercase().contains(&self.language.to_lowercase())
        {
            format!("{}: {} [{}]", self.id + 1, main_title, self.language.to_uppercase())
        } else {
            format!("{}: {}", self.id + 1, main_title)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_title() {
        assert_eq!(
            clean_title("[SubsPlease] Sousou no Frieren - 01 (1080p) [ABCD1234].mkv"),
            "Sousou no Frieren - 01"
        );
        assert_eq!(
            clean_title("[AniDUB] JoJo's Bizarre Adventure - 24 [1080p].mkv"),
            "JoJo's Bizarre Adventure - 24"
        );
        assert_eq!(
            clean_title("Yani.Neko.S01E05.1080p.WEB-DL.AAC.mkv"),
            "Yani Neko S01E05"
        );
        assert_eq!(
            clean_title("[get-save.com] Attack on Titan S04E15 [1080p].mp4"),
            "Attack on Titan S04E15"
        );
    }
}
