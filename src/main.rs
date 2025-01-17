extern crate byteorder;
extern crate getopts;
extern crate image;
extern crate sdfgen;

use std::fs::File;
use std::io::Write;

use image::GrayImage;

use getopts::Options;

use byteorder::{LittleEndian, WriteBytesExt};

use image::ImageEncoder;
use sdfgen::functions::bit_compressor;
use sdfgen::functions::bw_to_bits;
use sdfgen::sdf_algorithm::calculate_sdf;
use sdfgen::sdf_algorithm::sdf_to_grayscale_image;
use sdfgen::sdf_algorithm::DstT;

fn print_usage(program: &String, opts: &Options) {
    let brief = format!(
        "Usage: {} [options] inputimage.png outputimage.png",
        program
    );
    print!("{}", opts.usage(&brief));
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let program_name = args[0].clone();

    let mut opts = Options::new();
    opts.optflag("h", "help", "print help");
    opts.optflag("v", "verbose", "show what the program is doing");
    opts.optopt ("s","size","size of the output signed distance field image, must be a power of 2. Defaults to input size / 4","OUTPUT_SIZE");
    opts.optopt ( "","maxdst","saturation distance (i.e. 'most far away meaningful distance') in half pixels of the input image. Defaults to input size / 4","SATURATION_DISTANCE");
    opts.optopt ( "","save-mipmaps","save the mipmaps used for accelerated calculation to BASENAMEi.png, where 'i' is the mipmap level","BASENAME");
    opts.optopt ("t","type","One of 'png', 'png16', 'u16', 'f32', 'f64'. f32 and f64 are raw floating point formats, u16 is raw unsigned 16 bit integers. Default: png","TYPE");
    if args.len() == 1 {
        print_usage(&program_name, &opts);
        return;
    }
    let parsed_opts = match opts.parse(&args[1..]) {
        Ok(v) => v,
        Err(e) => {
            panic!("{}", e.to_string())
        }
    };
    if parsed_opts.opt_present("help") || parsed_opts.free.len() != 2 {
        print_usage(&program_name, &opts);
        return;
    }
    let input_image_name = &parsed_opts.free[0];
    let output_image_name = &parsed_opts.free[1];
    let verbose = parsed_opts.opt_present("verbose");

    if verbose {
        println!("Loading input image '{}'.", input_image_name);
    }
    let mut img: GrayImage = image::open(input_image_name)
        .expect("failed to load image")
        .to_luma8();
    {
        let (w, h) = img.dimensions();
        if verbose {
            println!("Image is of size {}x{} pixels.", w, h);
        }
        if w != h || w.count_ones() > 1 {
            let mut max_size = std::cmp::max(w, h);
            if max_size.count_ones() > 1 {
                max_size = 1_u32 << (32 - max_size.leading_zeros());
            }
            if verbose {
                println!("placing input in {}x{} canvas", max_size, max_size);
            }
            let offset_x = (max_size - w) / 2;
            let offset_y = (max_size - h) / 2;
            let fun = |x: u32, y: u32| -> image::Luma<u8> {
                let old_x = x.checked_sub(offset_x);
                let old_y = y.checked_sub(offset_y);
                // fill area around with white
                let mut v = 255_u8;
                if let (Some(old_x), Some(old_y)) = (old_x, old_y) {
                    if old_x < w && old_y < h {
                        v = img.get_pixel(old_x, old_y)[0]
                    }
                }
                image::Luma([v])
            };
            img = image::ImageBuffer::from_fn(max_size, max_size, fun);
        }
    }
    let (input_size, _) = img.dimensions();

    if verbose {
        println!("Converting image to binary.");
    }
    for px in img.pixels_mut() {
        px[0] = bw_to_bits(px[0]);
    }

    if verbose {
        println!("Calculating Mipmap.");
    }
    let mipmap = sdfgen::mipmap::Mipmap::new(img, bit_compressor);
    if verbose {
        println!("Mipmap has {} levels.", mipmap.get_max_level() + 1);
    }

    if parsed_opts.opt_present("save-mipmaps") {
        let basename = parsed_opts
            .opt_str("save-mipmaps")
            .expect("--save-mipmaps needs exactly one argument.");
        if verbose {
            println!(
                "Saving Mipmaps to {}[0..{}].png",
                basename,
                mipmap.get_max_level() + 1
            );
        }
        for i in 0..mipmap.get_max_level() + 1 {
            mipmap.images[i as usize]
                .save(format!("{}{}.png", basename, i))
                .unwrap();
        }
    }
    let sdf_size = match parsed_opts.opt_str("size") {
        Some(s) => s.parse::<u32>().unwrap(),
        None => input_size / 4,
    };
    let sat_dst: DstT = match parsed_opts.opt_str("maxdst") {
        Some(s) => s.parse::<DstT>().unwrap(),
        None => (input_size / 4) as DstT,
    };
    if verbose {
        println!(
            "Calculating signed distance field of size {} with saturation distance {}",
            sdf_size, sat_dst
        );
    }
    let mipmap_arc = std::sync::Arc::new(mipmap);
    let sdf = calculate_sdf(mipmap_arc, sdf_size);
    if verbose {
        println!("Doing a final color space conversion.");
    }
    let output_type = match parsed_opts.opt_str("type") {
        Some(s) => s,
        None => "png".to_string(),
    };
    match output_type.as_ref() {
        "png" => {
            let sdf_u8 = sdf_to_grayscale_image(&(*sdf), sat_dst);
            let (w, h) = sdf_u8.dimensions();
            // This design decision was done to be symmetric around 127
            let saturated_value = 254_u8;
            if verbose {
                let mut num_unsaturated_border_pixels = 0_usize;
                for x in 0..w {
                    if sdf_u8.get_pixel(x, 0)[0] < saturated_value {
                        num_unsaturated_border_pixels += 1;
                    }
                    if sdf_u8.get_pixel(x, h - 1)[0] < saturated_value {
                        num_unsaturated_border_pixels += 1;
                    }
                }
                for y in 0..h {
                    if sdf_u8.get_pixel(0, y)[0] < saturated_value {
                        num_unsaturated_border_pixels += 1;
                    }
                    if sdf_u8.get_pixel(w - 1, y)[0] < saturated_value {
                        num_unsaturated_border_pixels += 1;
                    }
                }
                println!(
                    "Unsaturated border pixels: {}",
                    num_unsaturated_border_pixels
                );
                println!(
                    "Saving {}x{} signed distance field image in png format as '{}'.",
                    w, h, output_image_name
                );
            }
            let outf = File::create(output_image_name).unwrap();
            let pngenc = image::codecs::png::PngEncoder::<std::fs::File>::new(outf);
            pngenc
                .write_image(sdf_u8.into_raw().as_ref(), w, h, image::ColorType::L8)
                .unwrap();
        }
        // TODO: remove code duplication here
        "u16" | "png16" => {
            let (w, h) = &sdf.dimensions();
            let mut buf = vec![];

            let writer = |b: &mut Vec<u8>, v| b.write_u16::<LittleEndian>(v);

            for px in sdf.into_raw() {
                let mut dst = px;
                dst = dst / sat_dst * 32767_f64;
                if dst < -32767_f64 {
                    dst = -32767_f64;
                } else if dst > 32767_f64 {
                    dst = 32767_f64;
                }
                debug_assert!(dst <= 32767_f64);
                debug_assert!(dst >= -32767_f64);
                let v: u16 = (dst as i32 + 32767) as u16;
                writer(&mut buf, v).unwrap();
            }
            if verbose {
                println!(
                    "Saving signed distance field image in u16 raw format as '{}'",
                    output_image_name
                );
            }

            let mut outf = File::create(output_image_name).unwrap();
            if output_type == "u16" {
                outf.write_all(buf.as_ref()).unwrap();
            } else {
                let pngenc = image::codecs::png::PngEncoder::<std::fs::File>::new(outf);
                pngenc
                    .write_image(buf.as_ref(), *w, *h, image::ColorType::L16)
                    .unwrap();
            }
        }
        "f64" => {
            let mut buf = vec![];
            for px in sdf.into_raw() {
                buf.write_f64::<LittleEndian>(px).unwrap();
            }
            if verbose {
                println!(
                    "Saving signed distance field image in f64 raw format as '{}'",
                    output_image_name
                );
            }
            let mut outf = File::create(output_image_name).unwrap();
            outf.write_all(buf.as_ref()).unwrap();
        }
        "f32" => {
            let mut buf = vec![];
            for px in sdf.into_raw() {
                buf.write_f32::<LittleEndian>(px as f32).unwrap();
            }
            if verbose {
                println!(
                    "Saving signed distance field image in f32 raw format as '{}'",
                    output_image_name
                );
            }
            let mut outf = File::create(output_image_name).unwrap();
            outf.write_all(buf.as_ref()).unwrap();
        }
        _ => {
            panic!("Unknown output format: {}", output_type);
        }
    };
}
