use game_music_emu::GameMusicEmu;
use std::env;

fn main() {
    let path = env::args().nth(1).expect("Usage: gme_tags <file>");
    let emu = GameMusicEmu::from_file(std::path::Path::new(&path), 44100)
        .expect("Failed to load file");

    let count = emu.track_count();
    println!("Track count: {}", count);

    for i in 0..count {
        match emu.track_info(i) {
            Ok(info) => {
                println!("\n--- Track {} ---", i);
                println!("  song:      '{}'", info.song);
                println!("  game:      '{}'", info.game);
                println!("  author:    '{}'", info.author);
                println!("  system:    '{}'", info.system);
                println!("  copyright: '{}'", info.copyright);
                println!("  comment:   '{}'", info.comment);
                println!("  dumper:    '{}'", info.dumper);
                println!("  play_length: {} ms", info.play_length);
            }
            Err(e) => println!("  Error: {}", e),
        }
    }
}
