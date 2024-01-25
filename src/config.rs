use serde::Deserialize;

#[derive(Deserialize)]
#[non_exhaustive]
pub struct Config {
    pub tmdb_api_key: String,

    #[serde(default)]
    pub user_agent: Option<String>,

    #[serde(default)]
    pub tmdb_path: Option<String>,
}

impl Config {
    pub fn user_agent(&self) -> &str {
        self.user_agent
            .as_deref()
            .unwrap_or("tvid/0.0 +https://github.com/agausmann/tvid")
    }
}
