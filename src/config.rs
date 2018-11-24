use std::collections::HashMap;
use std::default::Default;

#[derive(Deserialize)]
pub struct PlaybackFile {
    pub uri: String,
    #[serde(default)]
    pub looping: bool
}
#[derive(Deserialize)]
pub struct Config {
    pub files: HashMap<String, PlaybackFile>,
    pub listen: String,
    pub channels: Vec<String>,
    pub shutdown_secs: u64,
    pub sample_rate: u64
}
impl Config {
    pub fn get() -> Result<Self, ::failure::Error> {
        let mut settings = ::cfg::Config::default();
        settings
            .merge(::cfg::File::with_name("mfl-gramophone"))?
            .merge(::cfg::Environment::with_prefix("GRAMOPHONE"))?;
        let ret: Self = settings.try_into()?;
        Ok(ret)
    }
}
