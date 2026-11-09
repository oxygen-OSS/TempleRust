//! PC Speaker sound driver and TempleOS music macro player.
//!
//! Uses PIT channel 2 and the keyboard controller / system control port B (0x61).

#[inline]
unsafe fn inb(port: u16) -> u8 {
    let v: u8;
    core::arch::asm!("in al, dx", in("dx") port, out("al") v,
                     options(nostack, preserves_flags));
    v
}

#[inline]
unsafe fn outb(port: u16, val: u8) {
    core::arch::asm!("out dx, al", in("dx") port, in("al") val,
                     options(nostack, preserves_flags));
}

/// Set speaker frequency in Hz, or 0 to silence.
pub fn snd(freq: u32) {
    unsafe {
        if freq == 0 {
            let v = inb(0x61);
            outb(0x61, v & !0x03);
        } else {
            let divisor = (1_193_182 / freq).clamp(1, 65535) as u16;
            // Channel 2, lobyte/hibyte, mode 3 (square wave)
            outb(0x43, 0xb6);
            outb(0x42, (divisor & 0xff) as u8);
            outb(0x42, (divisor >> 8) as u8);
            let v = inb(0x61);
            outb(0x61, v | 0x03);
        }
    }
}

fn sleep_ms(ms: u64, tsc_per_ms: u64) {
    if tsc_per_ms == 0 {
        return;
    }
    let start = unsafe { core::arch::x86_64::_rdtsc() };
    let target = tsc_per_ms * ms;
    while unsafe { core::arch::x86_64::_rdtsc() }.wrapping_sub(start) < target {
        core::hint::spin_loop();
    }
}

/// Standard TempleOS beep: 500 Hz for 120 ms (or custom).
pub fn beep(freq: u32, duration_ms: u64, tsc_per_ms: u64) {
    let f = if freq == 0 { 500 } else { freq };
    let d = if duration_ms == 0 { 120 } else { duration_ms };
    snd(f);
    sleep_ms(d, tsc_per_ms);
    snd(0);
}

// Frequencies for octave 4 (C4..B4) in tenths of Hz
// C, C#, D, D#, E, F, F#, G, G#, A, A#, B
const OCTAVE_4_TENTHS: [u32; 12] = [
    2616, 2772, 2937, 3111, 3296, 3492, 3700, 3920, 4153, 4400, 4662, 4939,
];

fn note_to_semitone(ch: u8) -> Option<usize> {
    match ch.to_ascii_uppercase() {
        b'C' => Some(0),
        b'D' => Some(2),
        b'E' => Some(4),
        b'F' => Some(5),
        b'G' => Some(7),
        b'A' => Some(9),
        b'B' => Some(11),
        _ => None,
    }
}

/// Play a TempleOS music macro string, e.g. "O4 L8 C D E F G A B > C".
pub fn play(score: &str, tsc_per_ms: u64) {
    let bytes = score.as_bytes();
    let mut i = 0;
    let mut octave: i32 = 4;
    let mut default_len: u32 = 4; // quarter note
    let whole_note_ms: u64 = 1200;

    while i < bytes.len() {
        let b = bytes[i];
        i += 1;

        match b {
            b' ' | b'\t' | b'\n' | b'\r' | b',' => continue,
            b'>' => {
                if octave < 7 {
                    octave += 1;
                }
            }
            b'<' => {
                if octave > 1 {
                    octave -= 1;
                }
            }
            b'O' | b'o' => {
                if i < bytes.len() && bytes[i].is_ascii_digit() {
                    let o = (bytes[i] - b'0') as i32;
                    if (1..=7).contains(&o) {
                        octave = o;
                    }
                    i += 1;
                }
            }
            b'L' | b'l' => {
                let mut num = 0u32;
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    num = num * 10 + (bytes[i] - b'0') as u32;
                    i += 1;
                }
                if num > 0 && num <= 64 {
                    default_len = num;
                }
            }
            b'R' | b'r' | b'P' | b'p' => {
                // Rest
                let mut len = default_len;
                let mut num = 0u32;
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    num = num * 10 + (bytes[i] - b'0') as u32;
                    i += 1;
                }
                if num > 0 {
                    len = num;
                }
                let mut ms = whole_note_ms / (len as u64).max(1);
                if i < bytes.len() && bytes[i] == b'.' {
                    ms = ms * 3 / 2;
                    i += 1;
                }
                snd(0);
                sleep_ms(ms, tsc_per_ms);
            }
            _ => {
                if let Some(mut semi) = note_to_semitone(b) {
                    // Check accidental
                    if i < bytes.len() {
                        if bytes[i] == b'#' || bytes[i] == b'+' {
                            semi = (semi + 1) % 12;
                            i += 1;
                        } else if bytes[i] == b'-' {
                            semi = if semi == 0 { 11 } else { semi - 1 };
                            i += 1;
                        }
                    }

                    // Check explicit note length
                    let mut len = default_len;
                    let mut num = 0u32;
                    while i < bytes.len() && bytes[i].is_ascii_digit() {
                        num = num * 10 + (bytes[i] - b'0') as u32;
                        i += 1;
                    }
                    if num > 0 {
                        len = num;
                    }

                    // Dotted note
                    let mut ms = whole_note_ms / (len as u64).max(1);
                    if i < bytes.len() && bytes[i] == b'.' {
                        ms = ms * 3 / 2;
                        i += 1;
                    }

                    // Compute frequency
                    let base_tenths = OCTAVE_4_TENTHS[semi];
                    let freq = if octave >= 4 {
                        (base_tenths << (octave - 4)) / 10
                    } else {
                        (base_tenths >> (4 - octave)) / 10
                    };

                    let play_ms = ms * 85 / 100;
                    let gap_ms = ms - play_ms;

                    snd(freq);
                    sleep_ms(play_ms, tsc_per_ms);
                    snd(0);
                    if gap_ms > 0 {
                        sleep_ms(gap_ms, tsc_per_ms);
                    }
                }
            }
        }
    }
    snd(0);
}
