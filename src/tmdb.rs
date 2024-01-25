use std::{
    io::{Read, Write},
    rc::Rc,
};

use tmdb_client::{
    apis::{
        configuration::Configuration, ConfigurationApi, ConfigurationApiClient, SearchApi,
        SearchApiClient, TVSeasonsApi, TVSeasonsApiClient,
    },
    models::{self, Image, TvObject},
};

use crate::config::Config;

pub struct Tmdb {
    configuration: ConfigurationApiClient,
    search: SearchApiClient,
    seasons: TVSeasonsApiClient,
    server_config: Option<models::Configuration>,
}

impl Tmdb {
    pub fn new(config: &Config) -> Self {
        let mut api_config = Configuration::default();
        api_config.api_key = Some(config.tmdb_api_key.clone());
        api_config.user_agent = Some(config.user_agent().to_string());
        if let Some(base_path) = &config.tmdb_path {
            api_config.base_path = base_path.clone();
        }
        let api_config = Rc::new(api_config);

        Self {
            configuration: ConfigurationApiClient::new(Rc::clone(&api_config)),
            search: SearchApiClient::new(Rc::clone(&api_config)),
            seasons: TVSeasonsApiClient::new(Rc::clone(&api_config)),
            server_config: None,
        }
    }

    pub fn search(
        &self,
        query: &str,
        first_air_date_year: Option<i32>,
    ) -> Result<Vec<TvObject>, tmdb_client::Error> {
        self.search
            .get_search_tv_paginated(query, first_air_date_year, None, None)
            .map(|paginated| paginated.results.unwrap_or(Vec::new()))
    }

    pub fn get_season_images(
        &self,
        tv_id: i32,
        season_number: i32,
    ) -> Result<Vec<EpisodeImages>, tmdb_client::Error> {
        let details = self
            .seasons
            .get_tv_season_details(tv_id, season_number, None, None, None)?;

        let episodes = details
            .episodes
            .into_iter()
            .flatten()
            .map(|ep| EpisodeImages {
                episode_number: ep.episode_number.unwrap(),
                images: ep
                    .images
                    .into_iter()
                    .flat_map(|images| images.backdrops.unwrap_or(Vec::new()))
                    .collect(),
            })
            .collect();

        Ok(episodes)
    }

    pub fn get_image<W: Write>(
        &mut self,
        image: &Image,
        writer: &mut W,
    ) -> Result<u64, GetImageError> {
        let image_url = self.image_url(image).map_err(GetImageError::Tmdb)?;
        let response = ureq::get(&image_url).call().map_err(GetImageError::Ureq)?;
        let size_limit = 1 << 30; // 1 GiB;
        let mut reader = response.into_reader().take(size_limit);
        let written = std::io::copy(&mut reader, writer).map_err(GetImageError::Io)?;

        if written < size_limit {
            Ok(written)
        } else {
            Err(GetImageError::Overrun)
        }
    }

    fn image_url(&mut self, image: &Image) -> Result<String, tmdb_client::Error> {
        let config = self.server_config()?.images.as_ref().unwrap();
        Ok(format!(
            "{}/original/{}",
            config.base_url.as_ref().unwrap(),
            image.file_path.as_ref().unwrap()
        ))
    }

    fn server_config(&mut self) -> Result<&models::Configuration, tmdb_client::Error> {
        if self.server_config.is_none() {
            self.server_config = Some(self.configuration.get_configuration(None)?);
        }
        Ok(self.server_config.as_ref().unwrap())
    }
}

pub enum GetImageError {
    Tmdb(tmdb_client::Error),
    Ureq(ureq::Error),
    Io(std::io::Error),
    Overrun,
}

#[non_exhaustive]
pub struct EpisodeImages {
    pub episode_number: i32,
    pub images: Vec<Image>,
}
