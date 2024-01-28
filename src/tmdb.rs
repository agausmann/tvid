use std::io::Read;

use serde::Deserialize;
use ureq::{Agent, AgentBuilder, Request};

use crate::config::Config;

pub struct Tmdb {
    agent: Agent,
    api_key: String,
    server_config: Option<ServerConfig>,
}

impl Tmdb {
    pub fn new(config: &Config) -> Self {
        Self {
            agent: AgentBuilder::new().user_agent(&config.user_agent).build(),
            api_key: config.tmdb_api_key.clone(),
            server_config: None,
        }
    }

    pub fn search(&self, query: &str, year: Option<i32>) -> anyhow::Result<Vec<SearchResult>> {
        #[derive(Deserialize)]
        struct Response {
            results: Vec<SearchResult>,
        }
        let mut req = self.get("search/tv").query("query", query);
        if let Some(year) = year {
            req = req.query("year", &year.to_string());
        }
        let response: Response = req.call()?.into_json()?;
        Ok(response.results)
    }

    pub fn season_details(&self, tv_id: i32, season_number: i32) -> anyhow::Result<SeasonDetails> {
        let response = self
            .get(&format!("tv/{tv_id}/season/{season_number}"))
            .call()?
            .into_json()?;
        Ok(response)
    }

    pub fn episode_images(
        &self,
        tv_id: i32,
        season_number: i32,
        episode_number: i32,
    ) -> anyhow::Result<EpisodeImages> {
        let response = self
            .get(&format!(
                "tv/{tv_id}/season/{season_number}/episode/{episode_number}/images"
            ))
            .call()?
            .into_json()?;
        Ok(response)
    }

    pub fn get_image(
        &mut self,
        path: &str,
    ) -> anyhow::Result<Box<dyn Read + Send + Sync + 'static>> {
        let base_url = self.server_config()?.images.secure_base_url.clone();
        let reader = self
            .agent
            .get(&format!("{}/original/{}", base_url, path))
            .call()?
            .into_reader();

        Ok(reader)
    }

    fn server_config(&mut self) -> anyhow::Result<&ServerConfig> {
        if self.server_config.is_none() {
            self.server_config = Some(self.get("configuration").call()?.into_json()?);
        }
        Ok(self.server_config.as_ref().unwrap())
    }

    fn get(&self, path: &str) -> Request {
        self.agent
            .get(&format!("https://api.themoviedb.org/3/{}", path))
            .query("api_key", &self.api_key)
    }
}

#[derive(Deserialize)]
pub struct SearchResult {
    pub id: i32,
    pub name: String,
}

#[derive(Deserialize)]
pub struct SeasonDetails {
    pub episodes: Vec<SeasonEpisode>,
}

#[derive(Deserialize)]
pub struct SeasonEpisode {
    pub episode_number: i32,
}

#[derive(Deserialize)]
pub struct EpisodeImages {
    pub stills: Vec<Image>,
}

#[derive(Deserialize)]
pub struct Image {
    pub file_path: String,
}

#[derive(Deserialize)]
struct ServerConfig {
    images: ImagesConfig,
}

#[derive(Deserialize)]
struct ImagesConfig {
    secure_base_url: String,
}
