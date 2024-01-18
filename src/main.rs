use clap::Parser;
use ffmpeg_next as ffmpeg;
use image::{
    imageops::{flip_horizontal, flip_vertical, rotate90},
    ColorType, DynamicImage, GrayImage, RgbImage, RgbaImage,
};
use image_hasher::{HasherConfig, Image, ImageHash};
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::{stdout, BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};

#[derive(Debug, clap::Parser)]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Clone, clap::Subcommand)]
enum Command {
    Hash {
        video: PathBuf,
        #[clap(short, long)]
        output: Option<PathBuf>,
    },
    Compare {
        tvid: PathBuf,
        image: PathBuf,
    },
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    match &args.command {
        Command::Hash { video, output } => hash(video, output.as_deref()),
        Command::Compare { tvid, image } => compare(tvid, image),
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct CompareResult {
    distance: u32,
    frame: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct Tvid {
    hasher_config: HasherConfig,
    hashes: Vec<SerdeImageHash>,
}

#[derive(Debug, Serialize, Deserialize)]
struct SerdeImageHash(#[serde(with = "serde_image_hash")] ImageHash);

mod serde_image_hash {
    use image_hasher::ImageHash;
    use serde::{
        de::{Error, Unexpected},
        Deserialize, Deserializer, Serialize, Serializer,
    };

    pub fn serialize<S: Serializer>(v: &ImageHash, serializer: S) -> Result<S::Ok, S::Error> {
        v.to_base64().serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<ImageHash, D::Error> {
        String::deserialize(deserializer).and_then(|s| {
            ImageHash::from_base64(&s)
                .map_err(|_| D::Error::invalid_value(Unexpected::Str(&s), &"a base64 string"))
        })
    }
}

fn hash(video_path: &Path, output_path: Option<&Path>) -> anyhow::Result<()> {
    let hash_width = 8;
    let hash_height = 8;
    let hasher_config = HasherConfig::new()
        .hash_size(hash_width, hash_height)
        .resize_filter(image_hasher::FilterType::Triangle);
    let hasher = hasher_config.to_hasher();

    ffmpeg::init().unwrap();
    let mut ictx = ffmpeg::format::input(&video_path)?;
    let input = ictx
        .streams()
        .best(ffmpeg::media::Type::Video)
        .ok_or(ffmpeg::Error::StreamNotFound)?;
    let video_stream_index = input.index();

    let context_decoder = ffmpeg::codec::context::Context::from_parameters(input.parameters())?;
    let mut decoder = context_decoder.decoder().video()?;
    // let mut threading_config = decoder.threading();
    // threading_config.count = num_cpus::get();
    // threading_config.kind = ffmpeg::threading::Type::Slice;
    // decoder.set_threading(threading_config);

    let mut scaler = ffmpeg::software::scaling::context::Context::get(
        decoder.format(),
        decoder.width(),
        decoder.height(),
        ffmpeg::format::Pixel::RGB24,
        400,
        300,
        // decoder.width(),
        // decoder.height(),
        ffmpeg::software::scaling::flag::Flags::BILINEAR,
    )?;

    let mut frame_index = 0;

    let mut hashes = Vec::new();

    let mut receive_and_process_decoded_frames =
        |decoder: &mut ffmpeg::decoder::Video| -> Result<(), ffmpeg::Error> {
            let mut decoded = ffmpeg::util::frame::video::Video::empty();
            let mut rgb_frame = ffmpeg::util::frame::video::Video::empty();
            while decoder.receive_frame(&mut decoded).is_ok() {
                scaler.run(&decoded, &mut rgb_frame)?;
                dbg!(rgb_frame.width());
                dbg!(rgb_frame.height());
                dbg!(rgb_frame.stride(0));
                dbg!(rgb_frame.data(0).len());

                let mut packed =
                    vec![0u8; 3 * rgb_frame.width() as usize * rgb_frame.height() as usize];
                let src_stride = rgb_frame.stride(0) as usize;
                let dest_stride = 3 * rgb_frame.width() as usize;
                for row in 0..rgb_frame.height() as usize {
                    packed[row * dest_stride..][..dest_stride]
                        .copy_from_slice(&rgb_frame.data(0)[row * src_stride..][..dest_stride]);
                }
                let mut file = File::create(format!("data/debug/{}.ppm", frame_index)).unwrap();
                file.write_all(
                    format!("P6\n{} {}\n255\n", rgb_frame.width(), rgb_frame.height()).as_bytes(),
                )
                .unwrap();
                file.write_all(&packed).unwrap();

                let rgb_image =
                    RgbImage::from_raw(rgb_frame.width(), rgb_frame.height(), packed).unwrap();
                rgb_image
                    .save(format!("data/debug/{frame_index}.jpg"))
                    .unwrap();

                let frame_hash = hasher.hash_image(&rgb_image);
                hashes.push(SerdeImageHash(frame_hash));
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

    let tvid = Tvid {
        hasher_config,
        hashes,
    };

    match output_path {
        Some(path) => serde_json::to_writer(BufWriter::new(File::create(path)?), &tvid)?,
        None => serde_json::to_writer(stdout(), &tvid)?,
    }

    Ok(())
}

fn compare(tvid_path: &Path, image_path: &Path) -> anyhow::Result<()> {
    let tvid: Tvid = serde_json::from_reader(BufReader::new(File::open(tvid_path)?))?;
    let image = image::open(image_path)?;

    // let image = flip_vertical(&rotate90(&image));

    let hasher = tvid.hasher_config.to_hasher();
    let image_hash = hasher.hash_image(&image);

    let mut results: Vec<CompareResult> = tvid
        .hashes
        .iter()
        .enumerate()
        .map(|(frame, hash)| CompareResult {
            distance: hash.0.dist(&image_hash),
            frame: frame as u64,
        })
        .collect();

    dbg!(&results[3700..3800]);

    results.sort();

    dbg!(&results[..20]);
    dbg!(&results[results.len() - 20..]);

    Ok(())
}
