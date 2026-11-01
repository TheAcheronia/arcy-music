use lofty::file::{AudioFile, TaggedFileExt};
use lofty::probe::Probe;
use lofty::tag::Accessor;
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub fn sync_directory(conn: &mut Connection, dir_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let thumbs_dir = dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("arcy-music/thumbnails");
    
    fs::create_dir_all(&thumbs_dir)?;

    for entry in WalkDir::new(dir_path).into_iter().filter_map(|e| e.ok()) {
        if entry.file_type().is_file() {
            let path = entry.path();
            if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
                if matches!(ext.to_lowercase().as_str(), "mp3" | "flac" | "wav" | "ogg" | "m4a") {
                    process_file(conn, path, &thumbs_dir)?;
                }
            }
        }
    }
    
    Ok(())
}

fn process_file(conn: &mut Connection, path: &Path, thumbs_dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let path_str = path.to_string_lossy().to_string();
    
    // Hash path for ID
    let mut hasher = Sha256::new();
    hasher.update(path_str.as_bytes());
    let id = hasher.finalize().iter().map(|b| format!("{:02x}", b)).collect::<String>();
    
    // Check if already in DB (fast path)
    let exists: i64 = conn.query_row(
        "SELECT count(1) FROM tracks WHERE id = ?1",
        [&id],
        |row| row.get(0),
    ).unwrap_or(0);
    
    if exists > 0 {
        return Ok(());
    }

    // Read metadata
    let Ok(tagged_file) = Probe::open(path)
        .and_then(|probe| probe.read()) else {
        return Ok(());
    };

    let tag = tagged_file.primary_tag().or_else(|| tagged_file.first_tag());
    
    let title = tag.and_then(|t| t.title().map(String::from))
        .unwrap_or_else(|| path.file_stem().unwrap().to_string_lossy().to_string());
        
    let artist = tag.and_then(|t| t.artist().map(String::from));
    let album = tag.and_then(|t| t.album().map(String::from));
    
    let duration = tagged_file.properties().duration().as_secs() as i64;

    // Extract Thumbnail
    let mut thumbnail_url = None;
    if let Some(t) = tag {
        let pictures = t.pictures();
        if let Some(pic) = pictures.first() {
            let mut pic_hasher = Sha256::new();
            pic_hasher.update(pic.data());
            let pic_id = pic_hasher.finalize().iter().map(|b| format!("{:02x}", b)).collect::<String>();
            let ext = match pic.mime_type() {
                Some(mime) if mime.as_str() == "image/png" => "png",
                _ => "jpg",
            };
            let thumb_path = thumbs_dir.join(format!("{}.{}", pic_id, ext));
            if !thumb_path.exists() {
                // Check if user enabled mini_thumbnails
                let mini_enabled: Result<String, _> = conn.query_row(
                    "SELECT value FROM settings WHERE key = 'mini_thumbnails'",
                    [],
                    |row| row.get(0),
                );
                
                if mini_enabled.unwrap_or_else(|_| "0".to_string()) == "1" {
                    if let Ok(img) = image::load_from_memory(pic.data()) {
                        let resized = img.resize_exact(60, 60, image::imageops::FilterType::Triangle);
                        let _ = resized.save(&thumb_path);
                    } else {
                        fs::write(&thumb_path, pic.data())?;
                    }
                } else {
                    fs::write(&thumb_path, pic.data())?;
                }
            }
            thumbnail_url = Some(thumb_path.to_string_lossy().to_string());
        }
    }

    // Insert to DB
    conn.execute(
        "INSERT INTO tracks (id, title, artist, album, duration_secs, source_type, source_data, thumbnail_url)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        (
            &id,
            &title,
            artist,
            album,
            duration,
            "local",
            &path_str,
            thumbnail_url,
        ),
    )?;

    println!("Indexed: {}", title);
    Ok(())
}
