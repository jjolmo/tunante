use game_music_emu::GameMusicEmu;
fn main() {
    for p in std::env::args().skip(1) {
        let emu = match GameMusicEmu::from_file(std::path::Path::new(&p), 44100) { Ok(e)=>e, Err(e)=>{println!("{p}: {e:?}"); continue} };
        let i = emu.track_info(0).unwrap();
        println!("{:<44} length={:>7} intro={:>7} loop={:>7} play={:>7}",
            std::path::Path::new(&p).file_name().unwrap().to_string_lossy(),
            i.length, i.intro_length, i.loop_length, i.play_length);
    }
}
