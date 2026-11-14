use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use serde::Deserialize;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioExportFormat {
    Copy, // Lossless stream copy
    Mp3,  // High quality MP3 320k
    Flac, // Lossless FLAC
    Wav,  // Uncompressed PCM WAV
}

impl AudioExportFormat {
    pub fn extension(&self, native_codec: &str) -> &'static str {
        match self {
            AudioExportFormat::Copy => match native_codec.to_lowercase().as_str() {
                "aac" => "m4a",
                "ac3" | "eac3" => "ac3",
                "flac" => "flac",
                "opus" => "opus",
                "dts" => "dts",
                "mp3" => "mp3",
                "vorbis" => "ogg",
                _ => "mka",
            },
            AudioExportFormat::Mp3 => "mp3",
            AudioExportFormat::Flac => "flac",
            AudioExportFormat::Wav => "wav",
        }
    }

    pub fn display_label(&self) -> &'static str {
        match self {
            AudioExportFormat::Copy => "Оригинал (Lossless Copy)",
            AudioExportFormat::Mp3 => "MP3 (320 kbps)",
            AudioExportFormat::Flac => "FLAC (Lossless)",
            AudioExportFormat::Wav => "WAV (PCM)",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubtitleExportFormat {
    Ass, // .ass (Advanced SubStation Alpha: styles, colors, positions, karaoke, exact timings)
    Srt, // .srt (SubRip: universal text with timestamps)
    Vtt, // .vtt (WebVTT: for web and browsers)
}

impl SubtitleExportFormat {
    pub fn extension(&self) -> &'static str {
        match self {
            SubtitleExportFormat::Ass => "ass",
            SubtitleExportFormat::Srt => "srt",
            SubtitleExportFormat::Vtt => "vtt",
        }
    }

    pub fn display_label(&self) -> &'static str {
        match self {
            SubtitleExportFormat::Ass => ".ASS (Стили аниме, цвета и тайминги)",
            SubtitleExportFormat::Srt => ".SRT (Универсальный с таймингами)",
            SubtitleExportFormat::Vtt => ".VTT (WebVTT)",
        }
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ExtractedAudioTrack {
    pub stream_index: usize,
    pub relative_index: usize,
    pub title: String,
    pub language: String,
    pub codec: String,
    pub channels: Option<i32>,
    pub sample_rate: Option<String>,
    pub bit_rate: Option<u32>,
    pub is_default: bool,
}

impl ExtractedAudioTrack {
    pub fn display_name(&self) -> String {
        let studio = if !self.title.is_empty() {
            self.title.clone()
        } else {
            format!("Аудиодорожка #{}", self.relative_index + 1)
        };

        let lang_tag = if !self.language.is_empty() && self.language != "und" {
            format!(" [{}]", self.language.to_uppercase())
        } else {
            String::new()
        };

        let ch_str = match self.channels {
            Some(6) => " 5.1",
            Some(8) => " 7.1",
            Some(2) => " Stereo",
            Some(1) => " Mono",
            _ => "",
        };

        let br_str = if let Some(br) = self.bit_rate {
            format!(" • {} kbps", br / 1000)
        } else {
            String::new()
        };

        format!("{}{}{} ({}{})", studio, lang_tag, ch_str, self.codec.to_uppercase(), br_str)
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ExtractedSubtitleTrack {
    pub stream_index: usize,
    pub relative_index: usize,
    pub title: String,
    pub language: String,
    pub codec: String,
    pub is_default: bool,
    pub is_forced: bool,
}

impl ExtractedSubtitleTrack {
    pub fn display_name(&self) -> String {
        let name = if !self.title.is_empty() {
            self.title.clone()
        } else {
            format!("Субтитры #{}", self.relative_index + 1)
        };

        let lang_tag = if !self.language.is_empty() && self.language != "und" {
            format!(" [{}]", self.language.to_uppercase())
        } else {
            String::new()
        };

        let lower = self.codec.to_lowercase();
        let codec_str = match lower.as_str() {
            "ass" | "ssa" => "ASS (Стили и тайминги)",
            "subrip" | "srt" => "SRT",
            "webvtt" => "VTT",
            _ => self.codec.as_str(),
        };

        format!("{}{} ({})", name, lang_tag, codec_str)
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct MediaFileInfo {
    pub file_path: PathBuf,
    pub file_name: String,
    pub clean_title: String,
    pub file_size_bytes: u64,
    pub duration_secs: Option<f64>,
    pub container_format: String,
    pub audio_tracks: Vec<ExtractedAudioTrack>,
    pub subtitle_tracks: Vec<ExtractedSubtitleTrack>,
}

#[derive(Deserialize, Debug)]
struct FfprobeJson {
    streams: Option<Vec<FfprobeStreamJson>>,
    format: Option<FfprobeFormatJson>,
}

#[derive(Deserialize, Debug)]
struct FfprobeFormatJson {
    format_name: Option<String>,
    duration: Option<String>,
    size: Option<String>,
}

#[derive(Deserialize, Debug)]
struct FfprobeStreamJson {
    index: usize,
    codec_name: Option<String>,
    codec_type: Option<String>,
    channels: Option<i32>,
    sample_rate: Option<String>,
    bit_rate: Option<String>,
    disposition: Option<HashMap<String, i32>>,
    tags: Option<HashMap<String, String>>,
}

pub fn find_ffmpeg() -> Option<PathBuf> {
    // 1. Check in standard PATH
    let mut check_cmd = Command::new("ffmpeg");
    check_cmd.arg("-version");
    #[cfg(target_os = "windows")]
    check_cmd.creation_flags(CREATE_NO_WINDOW);

    if check_cmd.output().is_ok() {
        return Some(PathBuf::from("ffmpeg"));
    }

    // 2. Check standard Windows installations
    let paths = [
        r"C:\ffmpeg\bin\ffmpeg.exe",
        r"C:\Program Files\ffmpeg\bin\ffmpeg.exe",
        r"C:\Program Files (x86)\ffmpeg\bin\ffmpeg.exe",
    ];
    for p in paths {
        let pb = PathBuf::from(p);
        if pb.exists() {
            return Some(pb);
        }
    }

    None
}

pub fn find_ffprobe() -> Option<PathBuf> {
    let mut check_cmd = Command::new("ffprobe");
    check_cmd.arg("-version");
    #[cfg(target_os = "windows")]
    check_cmd.creation_flags(CREATE_NO_WINDOW);

    if check_cmd.output().is_ok() {
        return Some(PathBuf::from("ffprobe"));
    }

    let paths = [
        r"C:\ffmpeg\bin\ffprobe.exe",
        r"C:\Program Files\ffmpeg\bin\ffprobe.exe",
        r"C:\Program Files (x86)\ffmpeg\bin\ffprobe.exe",
    ];
    for p in paths {
        let pb = PathBuf::from(p);
        if pb.exists() {
            return Some(pb);
        }
    }

    None
}

pub fn inspect_file(path: &Path) -> Result<MediaFileInfo, String> {
    let ffprobe_bin = find_ffprobe().ok_or_else(|| {
        "ffprobe не найден в системе. Установите FFmpeg для детального анализа и извлечения дорожек.".to_string()
    })?;

    let mut cmd = Command::new(ffprobe_bin);
    cmd.arg("-v")
        .arg("quiet")
        .arg("-print_format")
        .arg("json")
        .arg("-show_streams")
        .arg("-show_format")
        .arg(path);

    #[cfg(target_os = "windows")]
    cmd.creation_flags(CREATE_NO_WINDOW);

    let output = cmd.output().map_err(|e| format!("Не удалось запустить ffprobe: {}", e))?;
    if !output.status.success() {
        return Err(format!("ffprobe завершился с ошибкой: {}", String::from_utf8_lossy(&output.stderr)));
    }

    let parsed: FfprobeJson = serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("Ошибка парсинга метаданных ffprobe: {}", e))?;

    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "media_file".to_string());
    let clean_title = crate::player::tracks::clean_title(&file_name);

    let file_size_bytes = std::fs::metadata(path)
        .map(|m| m.len())
        .unwrap_or_else(|_| {
            parsed
                .format
                .as_ref()
                .and_then(|f| f.size.as_ref())
                .and_then(|s| s.parse().ok())
                .unwrap_or(0)
        });

    let duration_secs = parsed
        .format
        .as_ref()
        .and_then(|f| f.duration.as_ref())
        .and_then(|d| d.parse().ok());

    let container_format = parsed
        .format
        .as_ref()
        .and_then(|f| f.format_name.clone())
        .unwrap_or_else(|| "unknown".to_string());

    let mut audio_tracks = Vec::new();
    let mut subtitle_tracks = Vec::new();

    let mut audio_counter = 0;
    let mut sub_counter = 0;

    if let Some(streams) = parsed.streams {
        for stream in streams {
            let codec_type = stream.codec_type.as_deref().unwrap_or_default();
            let codec_name = stream.codec_name.unwrap_or_else(|| "unknown".to_string());

            let mut title = String::new();
            let mut language = String::new();

            if let Some(ref tags) = stream.tags {
                for (k, v) in tags {
                    let k_lower = k.to_lowercase();
                    if k_lower == "title" && title.is_empty() {
                        title = v.clone();
                    } else if k_lower == "handler_name" && title.is_empty() {
                        title = v.clone();
                    } else if k_lower == "language" {
                        language = v.clone();
                    }
                }
            }

            let is_default = stream
                .disposition
                .as_ref()
                .and_then(|d| d.get("default").copied())
                .unwrap_or(0)
                == 1;
            let is_forced = stream
                .disposition
                .as_ref()
                .and_then(|d| d.get("forced").copied())
                .unwrap_or(0)
                == 1;

            if codec_type == "audio" {
                let bit_rate = stream.bit_rate.and_then(|b| b.parse().ok());
                audio_tracks.push(ExtractedAudioTrack {
                    stream_index: stream.index,
                    relative_index: audio_counter,
                    title,
                    language,
                    codec: codec_name,
                    channels: stream.channels,
                    sample_rate: stream.sample_rate,
                    bit_rate,
                    is_default,
                });
                audio_counter += 1;
            } else if codec_type == "subtitle" {
                subtitle_tracks.push(ExtractedSubtitleTrack {
                    stream_index: stream.index,
                    relative_index: sub_counter,
                    title,
                    language,
                    codec: codec_name,
                    is_default,
                    is_forced,
                });
                sub_counter += 1;
            }
        }
    }

    Ok(MediaFileInfo {
        file_path: path.to_path_buf(),
        file_name,
        clean_title,
        file_size_bytes,
        duration_secs,
        container_format,
        audio_tracks,
        subtitle_tracks,
    })
}

pub fn extract_audio(
    input_file: &Path,
    stream_index: usize,
    format: AudioExportFormat,
    output_file: &Path,
) -> Result<(), String> {
    let ffmpeg_bin = find_ffmpeg().ok_or_else(|| "ffmpeg не найден в системе".to_string())?;

    let mut cmd = Command::new(ffmpeg_bin);
    cmd.arg("-y")
        .arg("-i")
        .arg(input_file)
        .arg("-map")
        .arg(format!("0:{}", stream_index));

    match format {
        AudioExportFormat::Copy => {
            cmd.arg("-c:a").arg("copy");
        }
        AudioExportFormat::Mp3 => {
            cmd.arg("-c:a")
                .arg("libmp3lame")
                .arg("-b:a")
                .arg("320k");
        }
        AudioExportFormat::Flac => {
            cmd.arg("-c:a").arg("flac");
        }
        AudioExportFormat::Wav => {
            cmd.arg("-c:a").arg("pcm_s16le");
        }
    }

    cmd.arg(output_file);

    #[cfg(target_os = "windows")]
    cmd.creation_flags(CREATE_NO_WINDOW);

    let output = cmd.output().map_err(|e| format!("Ошибка выполнения ffmpeg: {}", e))?;
    if !output.status.success() {
        return Err(format!(
            "Ошибка экспорта аудио: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    Ok(())
}

pub fn extract_subtitle(
    input_file: &Path,
    stream_index: usize,
    source_codec: &str,
    format: SubtitleExportFormat,
    output_file: &Path,
) -> Result<(), String> {
    let ffmpeg_bin = find_ffmpeg().ok_or_else(|| "ffmpeg не найден в системе".to_string())?;

    let mut cmd = Command::new(ffmpeg_bin);
    cmd.arg("-y")
        .arg("-i")
        .arg(input_file)
        .arg("-map")
        .arg(format!("0:{}", stream_index));

    match format {
        SubtitleExportFormat::Ass => {
            // If source is already ASS/SSA, copy directly to preserve exact styling & tags
            if source_codec.to_lowercase().contains("ass") || source_codec.to_lowercase().contains("ssa") {
                cmd.arg("-c:s").arg("copy");
            } else {
                cmd.arg("-c:s").arg("ass");
            }
        }
        SubtitleExportFormat::Srt => {
            // Converts to clean SubRip text with exact millisecond timestamps
            cmd.arg("-c:s").arg("subrip");
        }
        SubtitleExportFormat::Vtt => {
            cmd.arg("-c:s").arg("webvtt");
        }
    }

    cmd.arg(output_file);

    #[cfg(target_os = "windows")]
    cmd.creation_flags(CREATE_NO_WINDOW);

    let output = cmd.output().map_err(|e| format!("Ошибка выполнения ffmpeg: {}", e))?;
    if !output.status.success() {
        return Err(format!(
            "Ошибка экспорта субтитров: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    Ok(())
}
