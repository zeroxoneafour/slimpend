use std::fmt;

#[derive(Debug)]
pub(crate) enum SlimpendError {
    DeviceNotFound(&'static str),
    InvalidWaveform,
}

impl fmt::Display for SlimpendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SlimpendError::DeviceNotFound(dev) => {
                write!(f, "{} not found", dev)
            }
            SlimpendError::InvalidWaveform => {
                write!(f, "Waveform is invalid")
            }
        }
    }
}

impl std::error::Error for SlimpendError {}
