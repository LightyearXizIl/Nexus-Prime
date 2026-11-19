use super::feedback::{decode_wav, Clip, Cue, CuePair};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const MIN_MS: u32 = 50;
const MAX_MS: u32 = 30_000;
const MAX_VOLUME_PERCENT: u8 = 150;
const MAX_SOURCE_BYTES: usize = 128 * 1024 * 1024;
const FRAMES_PER_MS: usize = 48;
const MAX_BYTES: usize = MAX_MS as usize * FRAMES_PER_MS * 4 + 44;
static IMPORT_LOCK: parking_lot::Mutex<()> = parking_lot::const_mutex(());

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Preset {
    #[default]
    Guitar,
    Soft,
    Glass,
    Pulse,
    Wood,
    Air,
    Switch,
    Notice,
    Crystal,
    Reply,
    Custom,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct ImportedCue {
    id: String,
    name: String,
    source_ms: u32,
    #[serde(default)]
    source_id: Option<String>,
    #[serde(default)]
    start_ms: u32,
    pub duration_ms: u32,
}

#[derive(Serialize)]
pub(crate) struct AudioSource {
    id: String,
    name: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum SourceMode {
    Preset,
    Custom,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct SongSegment {
    id: String,
    start_ms: u32,
    duration_ms: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct SongCues {
    source_id: String,
    name: String,
    source_ms: u32,
    start: SongSegment,
    end: SongSegment,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub(crate) struct VoiceFeedbackSettings {
    pub enabled: bool,
    preset: Preset,
    #[serde(skip_serializing_if = "Option::is_none")]
    volume_percent: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_mode: Option<SourceMode>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    songs: Vec<SongCues>,
    #[serde(skip_serializing_if = "Option::is_none")]
    active_song_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    song: Option<SongCues>,
    #[serde(skip_serializing_if = "Option::is_none")]
    start: Option<ImportedCue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    end: Option<ImportedCue>,
}

fn check_id(id: &str) -> Result<(), String> {
    if id.len() != 64 || !id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("invalid audio ID".into());
    }
    Ok(())
}

fn asset_path(dir: &Path, id: &str) -> Result<PathBuf, String> {
    check_id(id)?;
    Ok(dir.join("voice-feedback").join(format!("{id}.wav")))
}

fn source_path(dir: &Path, id: &str) -> Result<PathBuf, String> {
    check_id(id)?;
    Ok(dir
        .join("voice-feedback")
        .join("sources")
        .join(format!("{id}.audio")))
}

fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.is_empty() || bytes.len() > limit {
        return Err("audio file size outside the supported range".into());
    }
    Ok(bytes)
}

fn store(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let _lock = IMPORT_LOCK.lock();
    fs::create_dir_all(path.parent().ok_or("missing audio directory")?)
        .map_err(|e| e.to_string())?;
    let temporary = path.with_extension("tmp");
    fs::write(&temporary, bytes).map_err(|e| e.to_string())?;
    fs::rename(&temporary, path).map_err(|e| e.to_string())
}

pub(crate) fn import_source(dir: &Path, file: &Path) -> Result<AudioSource, String> {
    let bytes = read_bounded(file, MAX_SOURCE_BYTES)?;
    let id = format!("{:x}", Sha256::digest(&bytes));
    store(&source_path(dir, &id)?, &bytes)?;
    let name = file
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .chars()
        .filter(|ch| !ch.is_control())
        .take(120)
        .collect();
    Ok(AudioSource { id, name })
}

pub(crate) fn read_source(dir: &Path, id: &str) -> Result<Vec<u8>, String> {
    let source = source_path(dir, id)?;
    let bytes = if source.is_file() {
        read_bounded(&source, MAX_SOURCE_BYTES)?
    } else {
        // A clip imported by the initial duration-only UI is its own source.
        read_bounded(&asset_path(dir, id)?, MAX_BYTES)?
    };
    if format!("{:x}", Sha256::digest(&bytes)) != id {
        return Err("source audio failed verification".into());
    }
    Ok(bytes)
}

pub(crate) fn import(dir: &Path, name: &str, bytes: &[u8]) -> Result<ImportedCue, String> {
    if bytes.len() > MAX_BYTES {
        return Err("voice cue exceeds the import limit".into());
    }
    let clip = decode_wav(bytes)?;
    let source_ms = (clip.len() / FRAMES_PER_MS) as u32;
    if !(MIN_MS..=MAX_MS).contains(&source_ms) {
        return Err("voice cue must be 50–30000 milliseconds".into());
    }
    let id = format!("{:x}", Sha256::digest(bytes));
    let path = asset_path(dir, &id)?;
    store(&path, bytes)?;
    let name = name
        .chars()
        .filter(|ch| !ch.is_control())
        .take(120)
        .collect();
    Ok(ImportedCue {
        id,
        name,
        source_ms,
        source_id: None,
        start_ms: 0,
        duration_ms: source_ms,
    })
}

fn load_custom(dir: &Path, id: &str, duration_ms: u32) -> Result<Clip, String> {
    let bytes = read_bounded(&asset_path(dir, id)?, MAX_BYTES)?;
    if format!("{:x}", Sha256::digest(&bytes)) != id {
        return Err("imported audio file failed verification".into());
    }
    let clip = decode_wav(&bytes)?;
    let available_ms = (clip.len() / FRAMES_PER_MS) as u32;
    if available_ms < MIN_MS {
        return Err("imported voice cue is too short".into());
    }
    let frames = duration_ms.clamp(MIN_MS, available_ms.min(MAX_MS)) as usize * FRAMES_PER_MS;
    let mut samples = clip[..frames].to_vec();
    let fade_in = 3 * FRAMES_PER_MS;
    let fade_out = 12 * FRAMES_PER_MS;
    for (index, frame) in samples.iter_mut().enumerate() {
        let gain = (index as f32 / fade_in as f32)
            .min((frames - 1 - index) as f32 / fade_out as f32)
            .min(1.0);
        frame[0] *= gain;
        frame[1] *= gain;
    }
    Ok(Arc::new(samples))
}

pub(crate) fn preset_bytes(preset: Preset, cue: Cue) -> &'static [u8] {
    macro_rules! pair {
        ($start:literal, $end:literal) => {
            match cue {
                Cue::Start => include_bytes!(concat!("../../assets/voice-feedback/", $start)),
                Cue::End => include_bytes!(concat!("../../assets/voice-feedback/", $end)),
            }
        };
    }
    match preset {
        Preset::Guitar => pair!("start.wav", "end.wav"),
        Preset::Soft | Preset::Custom => pair!("soft-start.wav", "soft-end.wav"),
        Preset::Glass => pair!("glass-start.wav", "glass-end.wav"),
        Preset::Pulse => pair!("pulse-start.wav", "pulse-end.wav"),
        Preset::Wood => pair!("wood-start.wav", "wood-end.wav"),
        Preset::Air => pair!("air-start.wav", "air-end.wav"),
        Preset::Switch => pair!("switch-start.wav", "switch-end.wav"),
        Preset::Notice => pair!("notice-start.wav", "notice-end.wav"),
        Preset::Crystal => pair!("crystal-start.wav", "crystal-end.wav"),
        Preset::Reply => pair!("reply-start.wav", "reply-end.wav"),
    }
}

impl VoiceFeedbackSettings {
    pub(crate) fn remove_deleted_song_assets(&self, dir: &Path, saved: &Self) {
        let deleted = self
            .songs
            .iter()
            .chain(self.song.iter())
            .filter(|song| {
                !saved
                    .songs
                    .iter()
                    .chain(saved.song.iter())
                    .any(|entry| entry.source_id == song.source_id)
            })
            .collect::<Vec<_>>();
        if deleted.is_empty() {
            return;
        }

        let mut referenced = HashSet::new();
        for song in saved.songs.iter().chain(saved.song.iter()) {
            referenced.extend([song.source_id.as_str(), &song.start.id, &song.end.id]);
        }
        for cue in saved.start.iter().chain(saved.end.iter()) {
            referenced.insert(cue.id.as_str());
            referenced.extend(cue.source_id.as_deref());
        }

        let _guard = IMPORT_LOCK.lock();
        for song in deleted {
            let mut paths = Vec::new();
            if !referenced.contains(song.source_id.as_str()) {
                paths.push(source_path(dir, &song.source_id));
            }
            for id in [&song.source_id, &song.start.id, &song.end.id] {
                if !referenced.contains(id.as_str()) {
                    paths.push(asset_path(dir, id));
                }
            }
            for path in paths.into_iter().flatten() {
                if let Err(error) = fs::remove_file(path) {
                    if error.kind() != std::io::ErrorKind::NotFound {
                        log::warn!("Could not remove an unused voice cue file: {error}");
                    }
                }
            }
        }
    }

    pub(crate) fn clip(&self, dir: &Path, cue: Cue) -> Result<Clip, String> {
        let mut clip = self.source_clip(dir, cue)?;
        let gain = f32::from(self.volume_percent.unwrap_or(100).min(MAX_VOLUME_PERCENT)) / 100.0;
        if gain != 1.0 {
            for frame in Arc::make_mut(&mut clip) {
                frame[0] *= gain;
                frame[1] *= gain;
            }
        }
        Ok(clip)
    }

    fn source_clip(&self, dir: &Path, cue: Cue) -> Result<Clip, String> {
        if matches!(self.source_mode, Some(SourceMode::Custom)) {
            let song = self
                .songs
                .iter()
                .find(|song| self.active_song_id.as_deref() == Some(song.source_id.as_str()))
                .or_else(|| self.songs.first())
                .or(self.song.as_ref())
                .ok_or("no song selected")?;
            check_id(&song.source_id)?;
            let segment = match cue {
                Cue::Start => &song.start,
                Cue::End => &song.end,
            };
            if !(MIN_MS..=MAX_MS).contains(&segment.duration_ms)
                || segment.start_ms.saturating_add(segment.duration_ms) > song.source_ms
            {
                return Err("voice cue range lies outside the song".into());
            }
            return load_custom(dir, &segment.id, segment.duration_ms);
        }
        if self.source_mode.is_none() && matches!(self.preset, Preset::Custom) {
            let imported = match cue {
                Cue::Start => &self.start,
                Cue::End => &self.end,
            };
            if let Some(imported) = imported {
                return load_custom(dir, &imported.id, imported.duration_ms);
            }
        }
        decode_wav(preset_bytes(self.preset, cue))
    }

    pub(crate) fn prepare(&self, dir: &Path) -> Result<Option<CuePair>, String> {
        if !self.enabled || self.volume_percent == Some(0) {
            return Ok(None);
        }
        Ok(Some([
            self.clip(dir, Cue::Start)?,
            self.clip(dir, Cue::End)?,
        ]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_are_short_and_each_edge_has_distinct_samples() {
        for preset in [
            Preset::Soft,
            Preset::Glass,
            Preset::Pulse,
            Preset::Wood,
            Preset::Air,
            Preset::Switch,
            Preset::Notice,
            Preset::Crystal,
            Preset::Reply,
        ] {
            let start = decode_wav(preset_bytes(preset, Cue::Start)).unwrap();
            let end = decode_wav(preset_bytes(preset, Cue::End)).unwrap();
            assert!([&start, &end]
                .into_iter()
                .all(|clip| (50 * FRAMES_PER_MS..=160 * FRAMES_PER_MS).contains(&clip.len())));
            assert_ne!(start, end);
        }
    }

    #[test]
    fn voice_volume_preserves_legacy_level_and_applies_to_preview_and_playback() {
        let dir = Path::new("");
        let mut settings: VoiceFeedbackSettings =
            serde_json::from_str(r#"{"enabled":true,"preset":"guitar"}"#).unwrap();
        let baseline = decode_wav(preset_bytes(Preset::Guitar, Cue::Start)).unwrap();
        assert_eq!(settings.clip(dir, Cue::Start).unwrap(), baseline);

        settings.volume_percent = Some(40);
        let restored: VoiceFeedbackSettings =
            serde_json::from_slice(&serde_json::to_vec(&settings).unwrap()).unwrap();
        let preview = restored.clip(dir, Cue::Start).unwrap();
        let expected: Vec<[f32; 2]> = baseline
            .iter()
            .map(|frame| frame.map(|sample| sample * 0.4))
            .collect();
        assert_eq!(preview.as_slice(), expected);
        assert_eq!(restored.prepare(dir).unwrap().unwrap()[0], preview);

        settings.volume_percent = Some(150);
        let boosted = settings.clip(dir, Cue::Start).unwrap();
        assert!(boosted
            .iter()
            .zip(baseline.iter())
            .all(|(frame, original)| *frame == original.map(|sample| sample * 1.5)));

        settings.volume_percent = Some(0);
        assert!(settings.prepare(dir).unwrap().is_none());
        assert!(settings
            .clip(dir, Cue::Start)
            .unwrap()
            .iter()
            .all(|frame| *frame == [0.0, 0.0]));
    }

    #[test]
    fn saved_songs_restore_active_pair_and_accept_a_legacy_single_song() {
        let dir = std::env::temp_dir().join(format!("nexus-cue-songs-{}", std::process::id()));
        let first = import(&dir, "first.wav", preset_bytes(Preset::Guitar, Cue::Start)).unwrap();
        let second = import(&dir, "second.wav", preset_bytes(Preset::Guitar, Cue::End)).unwrap();
        let song = |audio: &ImportedCue, start_duration, end_duration| SongCues {
            source_id: audio.id.clone(),
            name: audio.name.clone(),
            source_ms: audio.source_ms,
            start: SongSegment {
                id: audio.id.clone(),
                start_ms: 0,
                duration_ms: start_duration,
            },
            end: SongSegment {
                id: audio.id.clone(),
                start_ms: 60,
                duration_ms: end_duration,
            },
        };
        let legacy = VoiceFeedbackSettings {
            enabled: true,
            source_mode: Some(SourceMode::Custom),
            song: Some(song(&first, 100, 140)),
            ..VoiceFeedbackSettings::default()
        };
        assert_eq!(
            legacy.clip(&dir, Cue::End).unwrap().len(),
            140 * FRAMES_PER_MS
        );

        let library = VoiceFeedbackSettings {
            songs: vec![song(&first, 100, 140), song(&second, 180, 220)],
            active_song_id: Some(second.id.clone()),
            song: None,
            ..legacy
        };
        let mut restored: VoiceFeedbackSettings =
            serde_json::from_slice(&serde_json::to_vec(&library).unwrap()).unwrap();
        let pair = restored.prepare(&dir).unwrap().unwrap();
        assert_eq!(
            pair.map(|clip| clip.len()),
            [180, 220].map(|ms| ms * FRAMES_PER_MS)
        );
        restored.active_song_id = Some(first.id.clone());
        let pair = restored.prepare(&dir).unwrap().unwrap();
        assert_eq!(
            pair.map(|clip| clip.len()),
            [100, 140].map(|ms| ms * FRAMES_PER_MS)
        );

        for cue in [&first, &second] {
            fs::remove_file(asset_path(&dir, &cue.id).unwrap()).unwrap();
        }
        fs::remove_dir(dir.join("voice-feedback")).unwrap();
        fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn removing_a_song_keeps_shared_clips_and_the_imported_original() {
        let dir = std::env::temp_dir().join(format!("nexus-cue-remove-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let original = dir.join("original.wav");
        let source_bytes = preset_bytes(Preset::Guitar, Cue::Start);
        fs::write(&original, source_bytes).unwrap();
        let imported = import_source(&dir, &original).unwrap();
        let shared = import(&dir, "", preset_bytes(Preset::Soft, Cue::Start)).unwrap();
        let removed_clip = import(&dir, "", preset_bytes(Preset::Soft, Cue::End)).unwrap();
        let segment = |cue: &ImportedCue| SongSegment {
            id: cue.id.clone(),
            start_ms: 0,
            duration_ms: cue.duration_ms,
        };
        let removed = SongCues {
            source_id: imported.id.clone(),
            name: imported.name,
            source_ms: 720,
            start: segment(&shared),
            end: segment(&removed_clip),
        };
        let kept = SongCues {
            source_id: shared.id.clone(),
            name: "shared.wav".into(),
            source_ms: 720,
            start: segment(&shared),
            end: segment(&shared),
        };
        let previous = VoiceFeedbackSettings {
            songs: vec![removed, kept.clone()],
            ..VoiceFeedbackSettings::default()
        };
        let saved = VoiceFeedbackSettings {
            songs: vec![kept],
            ..VoiceFeedbackSettings::default()
        };
        previous.remove_deleted_song_assets(&dir, &saved);

        let source = source_path(&dir, &imported.id).unwrap();
        let shared_path = asset_path(&dir, &shared.id).unwrap();
        assert!(!source.exists());
        assert!(!asset_path(&dir, &removed_clip.id).unwrap().exists());
        assert_eq!(fs::read(&original).unwrap(), source_bytes);
        assert!(shared_path.is_file());

        fs::remove_file(original).unwrap();
        fs::remove_file(shared_path).unwrap();
        fs::remove_dir(source.parent().unwrap()).unwrap();
        fs::remove_dir(dir.join("voice-feedback")).unwrap();
        fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn imported_audio_survives_reload_and_trims_to_saved_duration() {
        let dir = std::env::temp_dir().join(format!("nexus-cue-test-{}", std::process::id()));
        let mut cue = import(&dir, "guitar.wav", preset_bytes(Preset::Guitar, Cue::Start)).unwrap();
        assert_eq!(cue.source_ms, 720);
        cue.duration_ms = 155;
        let restored: ImportedCue =
            serde_json::from_slice(&serde_json::to_vec(&cue).unwrap()).unwrap();
        let samples = load_custom(&dir, &restored.id, restored.duration_ms).unwrap();
        assert_eq!(samples.len(), 155 * FRAMES_PER_MS);
        assert_eq!(*samples.last().unwrap(), [0.0, 0.0]);
        assert!(load_custom(&dir, "../test", restored.duration_ms).is_err());
        assert!(import(&dir, "fake.wav", b"not a wav").is_err());
        let path = asset_path(&dir, &cue.id).unwrap();
        fs::remove_file(&path).unwrap();
        fs::remove_dir(path.parent().unwrap()).unwrap();
        fs::remove_dir(dir).unwrap();
    }
}
