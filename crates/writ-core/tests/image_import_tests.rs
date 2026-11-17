use std::path::{Path, PathBuf};

use chrono::NaiveDate;
use writ_core::config::ImageFolder;
use writ_core::notes::image_import::{
    check_image_head, check_image_size, clipboard_image_name, copied_file_name,
    encode_reference_part, extension_for_mime, image_destination, image_folder_label,
    image_markdown, is_lone_web_address, link_safe_file_name, not_images_notice,
    numbered_file_name, place_image_file, reference_inside, saved_image_reference,
    ImageImportError, ImagePlacement, IMAGE_MAX_BYTES, IMAGE_MAX_MB, MAX_NUMBERED_NAMES,
};

const PNG_HEAD: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";

fn note_dir() -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(r"C:\Users\me\Writ\trips")
    } else {
        PathBuf::from("/Users/me/Writ/trips")
    }
}

fn folder(value: &str) -> ImageFolder {
    ImageFolder::parse(value).unwrap()
}

#[test]
fn a_file_inside_the_note_folder_is_referred_to_where_it_is() {
    let dir = note_dir();
    let file = dir.join("photos").join("day one.png");
    assert_eq!(
        place_image_file(&dir, &folder("assets"), &file, "image/png"),
        ImagePlacement::InPlace {
            reference: "photos/day%20one.png".to_string()
        }
    );
}

#[test]
fn a_file_outside_the_note_folder_is_copied_into_the_image_folder() {
    let dir = note_dir();
    let file = Path::new(&dir)
        .parent()
        .unwrap()
        .join("Screen Shot [1].PNG");
    assert_eq!(
        place_image_file(&dir, &folder("assets"), &file, "image/png"),
        ImagePlacement::Copy {
            folder: dir.join("assets"),
            name: "Screen-Shot-1.png".to_string()
        }
    );
}

#[test]
fn a_sibling_folder_sharing_a_prefix_is_outside() {
    let dir = note_dir();
    let sibling = PathBuf::from(format!("{}-old", dir.display())).join("a.png");
    assert_eq!(reference_inside(&dir, &sibling), None);
    assert_eq!(reference_inside(&dir, &dir), None);
}

#[test]
fn the_image_folder_nests_under_the_note_folder() {
    let dir = note_dir();
    assert_eq!(
        image_destination(&dir, &folder("media/shots")),
        dir.join("media").join("shots")
    );
    assert_eq!(image_destination(&dir, &folder("")), dir);
}

#[test]
fn a_saved_reference_spells_the_folder_with_forward_slashes() {
    assert_eq!(
        saved_image_reference(&folder("media/my shots"), "a.png"),
        "media/my%20shots/a.png"
    );
    assert_eq!(saved_image_reference(&folder(""), "a.png"), "a.png");
    assert_eq!(image_markdown("assets/a.png"), "![](assets/a.png)");
}

#[test]
fn reference_parts_escape_what_a_link_would_break_on() {
    assert_eq!(encode_reference_part("a b(c).png"), "a%20b%28c%29.png");
    assert_eq!(encode_reference_part("<x>"), "%3Cx%3E");
    assert_eq!(encode_reference_part("صورة.png"), "صورة.png");
}

#[test]
fn link_safe_names_keep_letters_in_any_script() {
    assert_eq!(link_safe_file_name("my photo (2).JPG"), "my-photo-2.jpg");
    assert_eq!(link_safe_file_name("صورة الرحلة.png"), "صورة-الرحلة.png");
    assert_eq!(link_safe_file_name("a__b.c.png"), "a__b.c.png");
    assert_eq!(link_safe_file_name("[]().png"), "image.png");
    assert_eq!(link_safe_file_name(".hidden"), "hidden");
    assert_eq!(link_safe_file_name(""), "image");
}

#[test]
fn a_long_name_is_cut_to_a_stem_every_filesystem_takes() {
    let long = format!("{}.png", "a".repeat(400));
    let safe = link_safe_file_name(&long);
    assert!(safe.len() < 120, "{}", safe.len());
    assert!(safe.ends_with(".png"));
}

#[test]
fn a_copy_without_an_extension_gets_the_one_its_bytes_call_for() {
    assert_eq!(copied_file_name("pasted", "image/jpeg"), "pasted.jpg");
    assert_eq!(extension_for_mime("image/svg+xml"), Some("svg"));
    assert_eq!(extension_for_mime("text/html"), None);
}

#[test]
fn a_copy_is_saved_with_the_extension_its_bytes_call_for() {
    assert_eq!(copied_file_name("a.gif", "image/png"), "a.png");
    assert_eq!(copied_file_name("Photo.JPEG", "image/jpeg"), "Photo.jpg");
    assert_eq!(
        copied_file_name("drawing.svg", "image/webp"),
        "drawing.webp"
    );
    assert_eq!(
        copied_file_name("scan.2024.10.01", "image/png"),
        "scan.2024.10.01.png"
    );
}

#[test]
fn every_saved_name_fits_a_file_name_with_its_clash_number() {
    let at = NaiveDate::from_ymd_opt(2026, 11, 17)
        .unwrap()
        .and_hms_opt(9, 5, 7)
        .unwrap();
    for stem in ["😀".repeat(200), "画".repeat(300), "a".repeat(400)] {
        for name in [
            copied_file_name(&format!("{stem}.png"), "image/webp"),
            clipboard_image_name(&stem, at, "webp"),
        ] {
            let last = numbered_file_name(&name, MAX_NUMBERED_NAMES);
            assert!(last.len() <= 255, "{} bytes: {last}", last.len());
            assert!(
                last.ends_with(&format!("-{MAX_NUMBERED_NAMES}.webp")),
                "{last}"
            );
        }
    }
}

#[test]
fn a_device_name_is_never_a_saved_name() {
    let at = NaiveDate::from_ymd_opt(2026, 11, 17)
        .unwrap()
        .and_hms_opt(9, 5, 7)
        .unwrap();
    assert_eq!(copied_file_name("CON.png", "image/png"), "CON_.png");
    assert_eq!(copied_file_name("nul.tar.gif", "image/gif"), "nul_.tar.gif");
    assert_eq!(copied_file_name("com1", "image/png"), "com1_.png");
    assert_eq!(link_safe_file_name("aux.png"), "aux_.png");
    assert_eq!(copied_file_name("console.png", "image/png"), "console.png");
    assert_eq!(
        clipboard_image_name("prn", at, "png"),
        "prn-20261117-090507.png"
    );
}

#[test]
fn a_clash_moves_the_name_to_a_number() {
    assert_eq!(numbered_file_name("shot.png", 1), "shot.png");
    assert_eq!(numbered_file_name("shot.png", 2), "shot-2.png");
    assert_eq!(numbered_file_name("shot", 3), "shot-3");
}

#[test]
fn a_clipboard_picture_is_named_after_the_file_and_the_time() {
    let at = NaiveDate::from_ymd_opt(2026, 11, 17)
        .unwrap()
        .and_hms_opt(9, 5, 7)
        .unwrap();
    assert_eq!(
        clipboard_image_name("Trip plan", at, "png"),
        "Trip-plan-20261117-090507.png"
    );
    assert_eq!(
        clipboard_image_name("", at, "jpg"),
        "image-20261117-090507.jpg"
    );
}

#[test]
fn an_image_over_the_limit_is_turned_away_with_the_limit_named() {
    assert!(check_image_size("a.png", IMAGE_MAX_BYTES).is_ok());
    let err = check_image_size("a.png", IMAGE_MAX_BYTES + 1).unwrap_err();
    assert_eq!(
        err,
        ImageImportError::TooLarge {
            name: "a.png".to_string(),
            limit_mb: IMAGE_MAX_MB
        }
    );
    assert_eq!(err.to_string(), "a.png is over the 16 MB limit for images");
}

#[test]
fn only_bytes_the_editor_draws_are_taken() {
    assert_eq!(check_image_head("a.png", PNG_HEAD), Ok("image/png"));
    assert_eq!(
        check_image_head("a.png", b"<html><body></body></html>"),
        Err(ImageImportError::NotAnImage {
            name: "a.png".to_string()
        })
    );
}

#[test]
fn the_folder_label_names_the_file_folder_as_a_dot() {
    assert_eq!(image_folder_label(&folder("")), ".");
    assert_eq!(image_folder_label(&folder("assets")), "assets");
}

#[test]
fn a_lone_web_address_is_told_from_other_text() {
    for text in [
        "https://example.com/cat.png",
        "  http://example.com/a?b=c#d\n",
    ] {
        assert!(is_lone_web_address(text), "{text:?}");
    }
    for text in [
        "",
        "see https://example.com/cat.png",
        "https://a.example/x https://b.example/y",
        "https://a.example/x\nhttps://b.example/y",
        "file:///Users/me/cat.png",
        "ftp://example.com/cat.png",
        "example.com/cat.png",
        "https://",
        "data:image/png;base64,AAAA",
    ] {
        assert!(!is_lone_web_address(text), "{text:?}");
    }
}

#[test]
fn copied_files_passed_over_are_counted_in_one_notice() {
    assert_eq!(not_images_notice(0), None);
    assert_eq!(
        not_images_notice(1).as_deref(),
        Some("1 file is not an image and was not pasted")
    );
    assert_eq!(
        not_images_notice(3).as_deref(),
        Some("3 files are not images and were not pasted")
    );
}

#[test]
fn each_refusal_says_what_stopped_the_image() {
    assert_eq!(
        ImageImportError::PictureTooLarge {
            limit_mb: IMAGE_MAX_MB
        }
        .to_string(),
        "The copied picture is over the 16 MB limit for images"
    );
    assert_eq!(
        ImageImportError::NoFreeName {
            name: "shot.png".to_string(),
            folder: "assets".to_string()
        }
        .to_string(),
        "Could not find a free name for shot.png in the folder assets"
    );
    assert_eq!(
        ImageImportError::FolderOutside {
            folder: "assets".to_string()
        }
        .to_string(),
        "The folder assets leads outside the file's folder"
    );
}
