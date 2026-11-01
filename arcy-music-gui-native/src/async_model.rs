use slint::{Model, ModelNotify, ModelTracker, SharedPixelBuffer, Image};
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::thread;
use std::sync::mpsc;
use arcy_music_core::Track as CoreTrack;
use crate::slint_generatedMainWindow::Track;
use image::imageops::FilterType;

pub struct AsyncTrackModel {
    tracks: RefCell<Vec<CoreTrack>>,
    original_tracks: RefCell<Vec<CoreTrack>>,
    filter_query: RefCell<String>,
    sort_mode: RefCell<String>,
    cache: RefCell<HashMap<String, Image>>,
    cache_queue: RefCell<VecDeque<String>>,
    pending: RefCell<HashMap<String, bool>>,
    notify: ModelNotify,
    req_tx: mpsc::Sender<(usize, String, String)>, // (row, track_id, path)
    timer: RefCell<Option<slint::Timer>>,
}

impl AsyncTrackModel {
    pub fn new(tracks: Vec<CoreTrack>) -> std::rc::Rc<Self> {
        let (req_tx, req_rx) = mpsc::channel::<(usize, String, String)>();
        let completed_queue = Arc::new(Mutex::new(Vec::new()));
        
        let model = std::rc::Rc::new(Self {
            original_tracks: RefCell::new(tracks.clone()),
            tracks: RefCell::new(tracks),
            filter_query: RefCell::new(String::new()),
            sort_mode: RefCell::new(String::from("custom")),
            cache: RefCell::new(HashMap::new()),
            cache_queue: RefCell::new(VecDeque::new()),
            pending: RefCell::new(HashMap::new()),
            notify: ModelNotify::default(),
            req_tx,
            timer: RefCell::new(None),
        });

        let completed_clone = completed_queue.clone();
        
        // Background dedicated image resizing thread
        thread::spawn(move || {
            for (row, track_id, path) in req_rx {
                let completed = completed_clone.clone();
                rayon::spawn(move || {
                    let dyn_img_opt = if path.starts_with("http://") || path.starts_with("https://") {
                        let thumbs_dir = dirs::data_dir()
                            .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
                            .join("arcy-music/thumbnails");
                        let _ = std::fs::create_dir_all(&thumbs_dir);

                        // Extract safe filename from URL
                        let raw_name = path.split('/').last().unwrap_or("thumb").split('?').next().unwrap_or("thumb");
                        let safe_name: String = raw_name.chars().filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-').collect();
                        let cached_path = thumbs_dir.join(format!("{}.jpg", safe_name));

                        if cached_path.exists() {
                            image::open(&cached_path).ok()
                        } else if let Ok(resp) = reqwest::blocking::get(&path) {
                            if let Ok(bytes) = resp.bytes() {
                                if let Ok(img) = image::load_from_memory(&bytes) {
                                    let _ = img.to_rgb8().save_with_format(&cached_path, image::ImageFormat::Jpeg);
                                    Some(img)
                                } else {
                                    None
                                }
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    } else {
                        let p = std::path::Path::new(&path);
                        image::open(p).ok()
                    };

                    if let Some(dyn_img) = dyn_img_opt {
                        let resized = dyn_img.resize_exact(68, 68, FilterType::Triangle);
                        let rgba = resized.into_rgba8();
                        let buffer = SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
                            rgba.as_raw(),
                            rgba.width(),
                            rgba.height(),
                        );
                        completed.lock().unwrap().push((row, track_id, buffer));
                    }
                });
            }
        });

        // Setup UI thread polling timer (every 40ms)
        let timer = slint::Timer::default();
        let model_weak = std::rc::Rc::downgrade(&model);
        timer.start(slint::TimerMode::Repeated, std::time::Duration::from_millis(40), move || {
            if let Some(m) = model_weak.upgrade() {
                let mut q = completed_queue.lock().unwrap();
                if !q.is_empty() {
                    let mut rows_to_notify = Vec::new();
                    {
                        let mut cache = m.cache.borrow_mut();
                        let mut queue = m.cache_queue.borrow_mut();
                        let mut pending = m.pending.borrow_mut();
                        
                        for (row, track_id, buffer) in q.drain(..) {
                            pending.remove(&track_id);
                            
                            if queue.len() >= 500 {
                                if let Some(oldest) = queue.pop_front() {
                                    cache.remove(&oldest);
                                }
                            }
                            
                            let slint_img = Image::from_rgba8(buffer);
                            cache.insert(track_id.clone(), slint_img);
                            queue.push_back(track_id);
                            rows_to_notify.push(row);
                        }
                    }
                    for row in rows_to_notify {
                        m.notify.row_changed(row);
                    }
                }
            }
        });
        *model.timer.borrow_mut() = Some(timer);

        model
    }

    fn apply_filter_and_sort(&self) {
        let q = self.filter_query.borrow().clone();
        let mode = self.sort_mode.borrow().clone();
        let orig = self.original_tracks.borrow();

        let mut list: Vec<CoreTrack> = if q.is_empty() {
            orig.clone()
        } else {
            orig.iter()
                .filter(|t| {
                    t.title.to_lowercase().contains(&q)
                        || t.artist.as_deref().unwrap_or("").to_lowercase().contains(&q)
                        || t.album.as_deref().unwrap_or("").to_lowercase().contains(&q)
                })
                .cloned()
                .collect()
        };

        match mode.as_str() {
            "title_asc" => list.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase())),
            "title_desc" => list.sort_by(|a, b| b.title.to_lowercase().cmp(&a.title.to_lowercase())),
            "artist_asc" => list.sort_by(|a, b| {
                let a_art = a.artist.as_deref().unwrap_or("").to_lowercase();
                let b_art = b.artist.as_deref().unwrap_or("").to_lowercase();
                a_art.cmp(&b_art)
            }),
            "artist_desc" => list.sort_by(|a, b| {
                let a_art = a.artist.as_deref().unwrap_or("").to_lowercase();
                let b_art = b.artist.as_deref().unwrap_or("").to_lowercase();
                b_art.cmp(&a_art)
            }),
            "duration_asc" => list.sort_by(|a, b| a.duration_secs.unwrap_or(0).cmp(&b.duration_secs.unwrap_or(0))),
            "duration_desc" => list.sort_by(|a, b| b.duration_secs.unwrap_or(0).cmp(&a.duration_secs.unwrap_or(0))),
            _ => {}
        }

        *self.tracks.borrow_mut() = list;
        self.notify.reset();
    }

    pub fn set_tracks(&self, new_tracks: Vec<CoreTrack>) {
        *self.original_tracks.borrow_mut() = new_tracks;
        *self.filter_query.borrow_mut() = String::new();
        self.apply_filter_and_sort();
    }

    pub fn filter_tracks(&self, query: &str) {
        *self.filter_query.borrow_mut() = query.trim().to_lowercase();
        self.apply_filter_and_sort();
    }

    pub fn sort_tracks(&self, mode: &str) {
        *self.sort_mode.borrow_mut() = mode.to_string();
        self.apply_filter_and_sort();
    }

    pub fn move_track(&self, idx: usize, direction: &str) -> Option<(usize, usize, String)> {
        let mut tracks = self.tracks.borrow_mut();
        let len = tracks.len();
        if len < 2 {
            return None;
        }
        let target_idx = match direction {
            "up" if idx > 0 => idx - 1,
            "down" if idx + 1 < len => idx + 1,
            _ => return None,
        };

        let moved_track_id = tracks[idx].id.clone();
        tracks.swap(idx, target_idx);

        // Also update original_tracks to reflect custom reorder
        let mut orig = self.original_tracks.borrow_mut();
        if let (Some(pos1), Some(pos2)) = (
            orig.iter().position(|t| t.id == tracks[target_idx].id),
            orig.iter().position(|t| t.id == tracks[idx].id),
        ) {
            orig.swap(pos1, pos2);
        }

        self.notify.reset();
        Some((idx, target_idx, moved_track_id))
    }

    pub fn remove_track(&self, track_id: &str) {
        self.tracks.borrow_mut().retain(|t| t.id != track_id);
        self.original_tracks.borrow_mut().retain(|t| t.id != track_id);
        self.notify.reset();
    }

    pub fn get_track(&self, idx: usize) -> Option<CoreTrack> {
        self.tracks.borrow().get(idx).cloned()
    }

    pub fn get_all_tracks(&self) -> Vec<CoreTrack> {
        self.tracks.borrow().clone()
    }

    pub fn len(&self) -> usize {
        self.tracks.borrow().len()
    }
}

impl Model for AsyncTrackModel {
    type Data = Track;

    fn row_count(&self) -> usize {
        self.tracks.borrow().len()
    }

    fn row_data(&self, row: usize) -> Option<Self::Data> {
        let tracks = self.tracks.borrow();
        let t = tracks.get(row)?;

        let cache = self.cache.borrow();
        let thumbnail = if let Some(img) = cache.get(&t.id) {
            img.clone()
        } else {
            // Not in cache. If not already pending, request it!
            if let Some(ref path) = t.thumbnail_url {
                let mut pending = self.pending.borrow_mut();
                if !pending.contains_key(&t.id) {
                    pending.insert(t.id.clone(), true);
                    let _ = self.req_tx.send((row, t.id.clone(), path.clone()));
                }
            }
            slint::Image::default()
        };

        Some(Track {
            id: t.id.clone().into(),
            title: t.title.clone().into(),
            artist: t.artist.clone().unwrap_or_else(|| "Unknown".to_string()).into(),
            duration: format!("{:02}:{:02}", t.duration_secs.unwrap_or(0) / 60, t.duration_secs.unwrap_or(0) % 60).into(),
            thumbnail,
        })
    }

    fn model_tracker(&self) -> &dyn ModelTracker {
        &self.notify
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_thumbnail_loading() {
        let test_path = dirs::data_dir().unwrap_or_else(|| std::path::PathBuf::from("/tmp")).join("arcy-music/thumbnails/test.jpg");
        let p = test_path.as_path();
        if !p.exists() {
            return;
        }
        let dyn_img = image::open(p).expect("Failed to open");
        let resized = dyn_img.resize_exact(68, 68, FilterType::Triangle);
        let rgba = resized.into_rgba8();
        println!("RGBA len: {}", rgba.as_raw().len());
        let buffer = SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
            rgba.as_raw(),
            rgba.width(),
            rgba.height(),
        );
        let img = Image::from_rgba8(buffer);
        println!("Image size: {:?}", img.size());
        assert!(img.size().width > 0);
    }
}

