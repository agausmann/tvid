use anyhow::Context;
use clap::Parser;
use fast_image_resize::{DynamicImageView, DynamicImageViewMut, ImageView, ImageViewMut, Resizer};
use ffmpeg_next as ffmpeg;
use image::{EncodableLayout, GrayImage};
use image_hasher::{HasherConfig, ImageHash};
use serde::{Deserialize, Serialize};
use std::{
    cmp::Ordering,
    fs::File,
    io::{stdout, BufReader, BufWriter, Write},
    num::NonZeroU32,
    path::{Path, PathBuf},
    str::FromStr,
};

#[derive(Debug, clap::Parser)]
struct Args {
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
    Hash(HashArgs),
    Compare { tvid: PathBuf, image: PathBuf },
}

#[derive(Debug, Clone, clap::Parser)]
struct HashArgs {
    video: PathBuf,
    #[clap(short, long)]
    crop_aspect: Option<Aspect>,
    #[clap(short, long)]
    output: Option<PathBuf>,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    match &args.command {
        Command::Hash(hash_args) => hash(hash_args),
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

fn hash(args: &HashArgs) -> anyhow::Result<()> {
    let hash_width = 8;
    let hash_height = 8;
    let hasher_config = HasherConfig::new()
        .hash_size(hash_width, hash_height)
        .resize_filter(image_hasher::FilterType::Triangle);
    let hasher = hasher_config.to_hasher();

    ffmpeg::init().unwrap();
    let mut ictx = ffmpeg::format::input(&args.video)?;
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

    let mut resizer = Resizer::new(fast_image_resize::ResizeAlg::Convolution(
        fast_image_resize::FilterType::Bilinear,
    ));
    let mut resized_image = GrayImage::new(hash_width, hash_height);

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
                // let mut file = File::create(format!("data/debug/{}.ppm", frame_index)).unwrap();
                // file.write_all(
                //     format!("P6\n{} {}\n255\n", rgb_frame.width(), rgb_frame.height()).as_bytes(),
                // )
                // .unwrap();
                // file.write_all(&packed).unwrap();

                let gray_image =
                    GrayImage::from_raw(crop_width, crop_height, pack_and_crop).unwrap();
                // gray_image
                //     .save(format!("data/debug/{frame_index}.jpg"))
                //     .unwrap();

                resizer
                    .resize(
                        &DynamicImageView::U8(
                            ImageView::from_buffer(
                                NonZeroU32::new(gray_image.width()).unwrap(),
                                NonZeroU32::new(gray_image.height()).unwrap(),
                                gray_image.as_bytes(),
                            )
                            .unwrap(),
                        ),
                        &mut DynamicImageViewMut::U8(
                            ImageViewMut::from_buffer(
                                NonZeroU32::new(resized_image.width()).unwrap(),
                                NonZeroU32::new(resized_image.height()).unwrap(),
                                resized_image.as_mut(),
                            )
                            .unwrap(),
                        ),
                    )
                    .unwrap();
                // resized_image
                //     .save(format!("data/debug/{frame_index}-resized.png"))
                //     .unwrap();

                let frame_hash = hasher.hash_image(&gray_image);
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

    match &args.output {
        Some(path) => serde_json::to_writer(BufWriter::new(File::create(path)?), &tvid)?,
        None => serde_json::to_writer(stdout(), &tvid)?,
    }

    Ok(())
}

fn compare(tvid_path: &Path, image_path: &Path) -> anyhow::Result<()> {
    let tvid: Tvid = serde_json::from_reader(BufReader::new(File::open(tvid_path)?))?;
    let image = image::open(image_path)?;

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
