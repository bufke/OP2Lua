//! The beep when a map has loaded (OPU's beep5.wav, embedded as the VB editor embeds it).

const BEEP: &[u8] = include_bytes!("../assets/beep5.wav");

/// The audio output, opened the first time a sound plays; nothing plays when there is no output device.
#[derive(Default)]
pub struct Sounds {
    sink: Option<rodio::MixerDeviceSink>,
    /// The sound playing, kept so it isn't cut off.
    player: Option<rodio::Player>,
    unavailable: bool,
}

impl Sounds {
    /// Plays the beep (opening the audio output the first time).
    pub fn map_loaded(&mut self) {
        if self.unavailable {
            return;
        }
        if self.sink.is_none() {
            match rodio::DeviceSinkBuilder::open_default_sink() {
                Ok(s) => self.sink = Some(s),
                Err(_) => {
                    self.unavailable = true;
                    return;
                }
            }
        }
        if let Some(sink) = &self.sink {
            self.player = rodio::play(sink.mixer(), std::io::Cursor::new(BEEP)).ok();
        }
    }
}
