use arcy_music_core::{Command, PlaybackState};
use mpris_server::{Metadata, PlaybackStatus, Player, Time, TrackId};
use tokio::sync::mpsc;

pub fn start_mpris_service(
    command_tx: mpsc::Sender<Command>,
    mut state_rx: mpsc::Receiver<PlaybackState>,
) {
    std::thread::spawn(move || {
        let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
            Ok(rt) => rt,
            Err(e) => {
                eprintln!("MPRIS: Failed to create Tokio runtime: {}", e);
                return;
            }
        };

        let local = tokio::task::LocalSet::new();
        local.block_on(&rt, async move {
            let player = match Player::builder("ArcyMusic")
                .can_play(true)
                .can_pause(true)
                .can_go_previous(true)
                .can_go_next(true)
                .can_control(true)
                .build()
                .await
            {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("MPRIS: D-Bus connection failed (is dbus-daemon running?): {}", e);
                    return;
                }
            };

            // Connect D-Bus signal callbacks to the command channel
            let tx1 = command_tx.clone();
            player.connect_play_pause(move |_| {
                let _ = tx1.try_send(Command::PlayPause);
            });

            let tx2 = command_tx.clone();
            player.connect_play(move |_| {
                let _ = tx2.try_send(Command::Play);
            });

            let tx3 = command_tx.clone();
            player.connect_pause(move |_| {
                let _ = tx3.try_send(Command::Pause);
            });

            let tx4 = command_tx.clone();
            player.connect_stop(move |_| {
                let _ = tx4.try_send(Command::Stop);
            });

            let tx5 = command_tx.clone();
            player.connect_next(move |_| {
                let _ = tx5.try_send(Command::Next);
            });

            let tx6 = command_tx.clone();
            player.connect_previous(move |_| {
                let _ = tx6.try_send(Command::Previous);
            });

            let tx7 = command_tx.clone();
            player.connect_set_volume(move |_, vol| {
                let _ = tx7.try_send(Command::SetVolume(vol as f32));
            });

            let tx8 = command_tx.clone();
            player.connect_seek(move |_, offset| {
                let secs = offset.as_micros() as f32 / 1_000_000.0;
                let _ = tx8.try_send(Command::Seek(secs));
            });

            // Spawn the MPRIS server runner
            tokio::task::spawn_local(player.run());

            println!("MPRIS2 D-Bus service registered as org.mpris.MediaPlayer2.ArcyMusic");

            // Loop and process state updates to publish metadata/status to D-Bus
            while let Some(state) = state_rx.recv().await {
                let status = if state.is_playing {
                    PlaybackStatus::Playing
                } else if state.current_track.is_some() {
                    PlaybackStatus::Paused
                } else {
                    PlaybackStatus::Stopped
                };

                let _ = player.set_playback_status(status).await;
                let _ = player.set_volume(state.volume as f64).await;

                if let Some(ref track) = state.current_track {
                    let mut meta = Metadata::new();
                    meta.set_trackid(TrackId::try_from(format!("/org/mpris/MediaPlayer2/track/{}", track.id.replace(|c: char| !c.is_alphanumeric(), "_"))).ok());
                    meta.set_title(Some(&track.title));
                    if let Some(ref artist) = track.artist {
                        meta.set_artist(Some([artist.as_str()]));
                    }
                    if let Some(ref album) = track.album {
                        meta.set_album(Some(album.as_str()));
                    }
                    if let Some(duration) = track.duration_secs {
                        meta.set_length(Some(Time::from_micros((duration * 1_000_000) as i64)));
                    }
                    if let Some(ref thumb) = track.thumbnail_url {
                        meta.set_art_url(Some(thumb.as_str()));
                    }
                    let _ = player.set_metadata(meta).await;
                }
            }
        });
    });
}
