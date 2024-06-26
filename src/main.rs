use anyhow::{anyhow, Context};
use clap::Parser;
use ffmpeg_next as ffmpeg;
use image::GrayImage;
use serde::{Deserialize, Serialize};
use std::{
    cmp::Ordering,
    collections::HashMap,
    fs::File,
    io::{stdout, BufReader, BufWriter, Read},
    path::{Path, PathBuf},
    str::FromStr,
};
use tvid::{tmdb::Tmdb, GradientHash, Hash};

#[derive(Debug, clap::Parser)]
struct Args {
    #[clap(short, long)]
    config: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Clone, Copy)]
struct Aspect {
    width: u32,
    height: u32,
}

impl FromStr for Aspect {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (width, height) = s.split_once(":").context("missing colon in aspect ratio")?;
        Ok(Aspect {
            width: width.parse().context("width is not a valid integer")?,
            height: height.parse().context("height is not a valid integer")?,
        })
    }
}

#[derive(Debug, Clone, clap::Subcommand)]
enum Command {
    Search(SearchArgs),
    Fetch(FetchArgs),
    Hash(HashArgs),
    Identify(IdentifyArgs),
    Compare { tvid: PathBuf, image: PathBuf },
}

#[derive(Debug, Clone, clap::Parser)]
struct SearchArgs {
    /// Filter by the year that the show aired.
    ///
    /// This can be any year where an episode aired; it does not have to be the
    /// year of the first air date.
    #[clap(short, long)]
    year: Option<i32>,

    query: String,
}

#[derive(Debug, Clone, clap::Parser)]
struct FetchArgs {
    tvid: i32,
    season: i32,
}

#[derive(Debug, Clone, clap::Parser)]
struct HashArgs {
    video: PathBuf,
    #[clap(short, long)]
    crop_aspect: Option<Aspect>,
    #[clap(short, long)]
    output: Option<PathBuf>,
}

#[derive(Debug, Clone, clap::Parser)]
struct IdentifyArgs {
    tvid: PathBuf,
    tvds: PathBuf,

    // Maximum episode number
    #[clap(short('m'), long)]
    min: Option<i32>,

    // Minimum episode number
    #[clap(short('M'), long)]
    max: Option<i32>,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let raw_config =
        std::fs::read_to_string(args.config.as_deref().unwrap_or(Path::new("tvid.toml")))?;
    let config: tvid::config::Config = toml::from_str(&raw_config)?;

    match &args.command {
        Command::Search(search_args) => search(&config, search_args),
        Command::Fetch(fetch_args) => fetch(&config, fetch_args),
        Command::Hash(hash_args) => hash(hash_args),
        Command::Identify(identify_args) => identify(identify_args),
        Command::Compare { tvid, image } => compare(tvid, image),
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct IdResult {
    mse: u32,
    episode: i32,
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct CompareResult {
    distance: u32,
    frame: u64,
    hash: Hash,
}

#[derive(Debug, Serialize, Deserialize)]
struct Tvid {
    hashes: Vec<Hash>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Tvds {
    episodes: HashMap<i32, Episode>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Episode {
    thumbnails: Vec<Hash>,
}

fn search(config: &tvid::config::Config, search_args: &SearchArgs) -> anyhow::Result<()> {
    let tmdb = Tmdb::new(config);
    let results = tmdb
        .search(&search_args.query, search_args.year)
        .map_err(|e| anyhow!("api error: {e:?}"))?;

    for r in &results {
        println!("{:8} - {}", r.id, r.name);
    }

    Ok(())
}

fn fetch(config: &tvid::config::Config, fetch_args: &FetchArgs) -> anyhow::Result<()> {
    let mut tmdb = Tmdb::new(config);

    let season = tmdb.season_details(fetch_args.tvid, fetch_args.season)?;

    let mut hasher = GradientHash::new();

    let tvds = Tvds {
        episodes: season
            .episodes
            .into_iter()
            .map(|ep| {
                Ok((
                    ep.episode_number,
                    Episode {
                        thumbnails: tmdb
                            .episode_images(fetch_args.tvid, fetch_args.season, ep.episode_number)?
                            .stills
                            .into_iter()
                            .map(|image_ref| -> anyhow::Result<Hash> {
                                let mut image_data = Vec::new();
                                let image_reader = tmdb.get_image(&image_ref.file_path)?;
                                image_reader.take(1 << 30).read_to_end(&mut image_data)?;
                                let image = image::load_from_memory(&image_data)?;
                                let gray_image = image.into_luma8();

                                Ok(hasher.hash(&gray_image))
                            })
                            .flat_map(|result| match result {
                                Ok(x) => Some(x),
                                Err(e) => {
                                    eprintln!("error loading image: {:?}", e);
                                    None
                                }
                            })
                            .collect(),
                    },
                ))
            })
            .collect::<anyhow::Result<_>>()?,
    };

    serde_json::to_writer(
        BufWriter::new(File::create(format!(
            "{}s{:02}.tvds",
            fetch_args.tvid, fetch_args.season
        ))?),
        &tvds,
    )?;

    Ok(())
}

fn hash(args: &HashArgs) -> anyhow::Result<()> {
    ffmpeg::init().unwrap();
    let mut ictx = ffmpeg::format::input(&args.video)?;
    let input = ictx
        .streams()
        .best(ffmpeg::media::Type::Video)
        .ok_or(ffmpeg::Error::StreamNotFound)?;
    let video_stream_index = input.index();

    let context_decoder = ffmpeg::codec::context::Context::from_parameters(input.parameters())?;
    let mut decoder = context_decoder.decoder().video()?;

    let mut scaler = ffmpeg::software::scaling::context::Context::get(
        decoder.format(),
        decoder.width(),
        decoder.height(),
        ffmpeg::format::Pixel::GRAY8,
        decoder.width(),
        decoder.height(),
        ffmpeg::software::scaling::flag::Flags::BILINEAR,
    )?;

    let (crop_x, crop_y, crop_width, crop_height) = match args.crop_aspect.map(|aspect| {
        (
            aspect,
            (decoder.width() * aspect.height).cmp(&(decoder.height() * aspect.width)),
        )
    }) {
        Some((aspect, Ordering::Less)) => {
            // Crop top and bottom
            let target_height = decoder.width() * aspect.height / aspect.width;
            (
                0,
                (decoder.height() - target_height) / 2,
                decoder.width(),
                target_height,
            )
        }
        Some((aspect, Ordering::Greater)) => {
            // Crop left and right
            let target_width = decoder.height() * aspect.width / aspect.height;
            (
                (decoder.width() - target_width) / 2,
                0,
                target_width,
                decoder.height(),
            )
        }
        Some((_, Ordering::Equal)) | None => (0, 0, decoder.width(), decoder.height()),
    };

    let mut hasher = GradientHash::new();

    let mut frame_index = 0;

    let mut hashes = Vec::new();

    let mut receive_and_process_decoded_frames =
        |decoder: &mut ffmpeg::decoder::Video| -> Result<(), ffmpeg::Error> {
            let mut decoded = ffmpeg::util::frame::video::Video::empty();
            let mut gray_frame = ffmpeg::util::frame::video::Video::empty();
            while decoder.receive_frame(&mut decoded).is_ok() {
                scaler.run(&decoded, &mut gray_frame)?;

                let mut pack_and_crop = vec![0u8; (crop_width * crop_height) as usize];
                let src_stride = gray_frame.stride(0) as usize;
                let dest_stride = crop_width as usize;
                for row in 0..crop_height as usize {
                    pack_and_crop[row * dest_stride..][..dest_stride].copy_from_slice(
                        &gray_frame.data(0)
                            [(row + crop_y as usize) * src_stride + (crop_x as usize)..]
                            [..dest_stride],
                    );
                }
                let gray_image =
                    GrayImage::from_raw(crop_width, crop_height, pack_and_crop).unwrap();

                hashes.push(hasher.hash(&gray_image));
                dbg!(frame_index);
                frame_index += 1;
            }
            Ok(())
        };

    for (stream, packet) in ictx.packets() {
        if stream.index() == video_stream_index {
            decoder.send_packet(&packet)?;
            receive_and_process_decoded_frames(&mut decoder)?;
        }
    }
    decoder.send_eof()?;
    receive_and_process_decoded_frames(&mut decoder)?;

    let tvid = Tvid { hashes };

    match &args.output {
        Some(path) => serde_json::to_writer(BufWriter::new(File::create(path)?), &tvid)?,
        None => serde_json::to_writer(stdout(), &tvid)?,
    }

    Ok(())
}

fn identify(identify_args: &IdentifyArgs) -> anyhow::Result<()> {
    let tvid: Tvid = serde_json::from_reader(BufReader::new(File::open(&identify_args.tvid)?))?;
    let tvds: Tvds = serde_json::from_reader(BufReader::new(File::open(&identify_args.tvds)?))?;

    let mut result: Vec<IdResult> = tvds
        .episodes
        .into_iter()
        .filter(|&(ep_id, _)| {
            identify_args.min.map(|min| ep_id >= min).unwrap_or(true)
                && identify_args.max.map(|max| ep_id <= max).unwrap_or(true)
        })
        .map(|(ep_id, ep)| {
            let squared_error: u32 = ep
                .thumbnails
                .iter()
                .map(|thumb_hash| {
                    let distance = tvid
                        .hashes
                        .iter()
                        .map(|tv_hash| tv_hash.distance(&thumb_hash))
                        .min()
                        .unwrap();

                    eprintln!("{} {}", ep_id, distance);
                    distance * distance
                })
                .sum();
            let mse = squared_error * 1000 / (ep.thumbnails.len() as u32);
            IdResult {
                mse,
                episode: ep_id,
            }
        })
        .collect();

    result.sort();

    for r in result {
        println!("{:?}", r);
    }

    Ok(())
}

fn compare(tvid_path: &Path, image_path: &Path) -> anyhow::Result<()> {
    let tvid: Tvid = serde_json::from_reader(BufReader::new(File::open(tvid_path)?))?;
    let gray_image = image::open(image_path)?.to_luma8();

    let mut hasher = GradientHash::new();

    let image_hash = hasher.hash(&gray_image);
    println!("base {:02x?}", image_hash);

    let mut results: Vec<CompareResult> = tvid
        .hashes
        .iter()
        .enumerate()
        .map(|(frame, hash)| CompareResult {
            hash: *hash,
            distance: image_hash.distance(&hash),
            frame: frame as u64,
        })
        .collect();

    results.sort();

    for result in &results[..20] {
        println!(
            "{:02x?} dist {} frame {}",
            result.hash, result.distance, result.frame
        );
    }

    Ok(())
}
