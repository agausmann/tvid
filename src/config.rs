use serde::Deserialize;

#[derive(Deserialize)]
#[non_exhaustive]
pub struct Config {
    pub tmdb_api_key: String,
    pub user_agent: String,
}
