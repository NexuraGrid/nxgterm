//! Kitty graphics protocol (`ESC _ G … ESC \`).
//!
//! Spec: <https://sw.kovidgoyal.net/kitty/graphics-protocol/>. Supported:
//! transmit (`a=t`), transmit and display (`a=T`), place (`a=p`), delete
//! (`a=d`) and query (`a=q`); formats 24, 32 and 100 (PNG); direct,
//! file and temp-file transmission, chunking and zlib compression.
//! Shared memory (`t=s`) and animation are refused.
//!
//! TODO: unicode placeholders (`U=1`) are stored but not displayed; they
//! are what makes images work inside multiplexers (ngmux, tmux).

pub mod command;
mod file;

pub use command::Command;

use crate::image::decode::{self, Base64, DecodeError, MAX_DATA};
use crate::image::{ImageStore, Placement, SrcRect};
use crate::size::CellPixels;

/// Terminal state a command acts on.
#[derive(Debug)]
pub struct Context<'a> {
    pub store: &'a mut ImageStore,
    /// Cursor column and row.
    pub cursor: (u32, i32),
    pub cell: CellPixels,
}

/// What the terminal must do after a command.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    /// Reply for the child, already framed as an APC.
    pub reply: Option<Vec<u8>>,
    /// Move the cursor right by `.0` columns and down by `.1 - 1` rows
    /// (the placement's span).
    pub advance: Option<(u32, u32)>,
}

/// A chunked direct transmission in progress.
#[derive(Debug)]
struct Upload {
    cmd: Command,
    base64: Base64,
    data: Vec<u8>,
    error: Option<String>,
}

/// Protocol state across commands (chunked uploads).
#[derive(Debug, Default)]
pub struct Graphics {
    loading: Option<Upload>,
}

/// A failed command: an errno-style code plus a message.
type Failure = (&'static str, String);

impl Graphics {
    pub fn new() -> Self {
        Self::default()
    }

    /// Runs one command; `body` is the APC payload after `G`.
    pub fn handle(&mut self, body: &[u8], ctx: &mut Context<'_>) -> Outcome {
        let cmd = match Command::parse(body) {
            Ok(cmd) => cmd,
            Err(message) => {
                // Parsing failed, but the id may still be readable.
                let cmd = Command {
                    id: lenient_id(body),
                    ..Command::default()
                };
                return reply(&cmd, Err(("EINVAL", message)), None);
            }
        };
        match cmd.action {
            b't' | b'T' | b'q' => self.transmit(cmd, ctx),
            b'p' => {
                let found = find(&cmd, ctx.store);
                match found {
                    Ok(key) => {
                        let advance = place(&cmd, key, ctx);
                        let id = ctx.store.image(key).map_or(cmd.id, |i| i.id);
                        reply(&cmd, Ok(()), Some(id)).with_advance(advance)
                    }
                    Err(failure) => reply(&cmd, Err(failure), None),
                }
            }
            b'd' => {
                delete(&cmd, ctx);
                Outcome::default()
            }
            _ => reply(&cmd, Err(("EINVAL", "unsupported action".into())), None),
        }
    }

    fn transmit(&mut self, cmd: Command, ctx: &mut Context<'_>) -> Outcome {
        if cmd.medium == b'd' {
            if let Some(mut upload) = self.loading.take() {
                // A continuation chunk: only `m` (and `q`) matter.
                upload.append(&cmd.payload);
                if cmd.more {
                    self.loading = Some(upload);
                    return Outcome::default();
                }
                return upload.finish(ctx);
            }
            if cmd.more {
                let mut upload = Upload::new(cmd);
                let payload = std::mem::take(&mut upload.cmd.payload);
                upload.append(&payload);
                self.loading = Some(upload);
                return Outcome::default();
            }
        }
        let mut upload = Upload::new(cmd);
        match upload.cmd.medium {
            b'd' => {
                let payload = std::mem::take(&mut upload.cmd.payload);
                upload.append(&payload);
            }
            b'f' | b't' => match file::read(&upload.cmd) {
                Ok(data) => upload.data = data,
                Err(message) => upload.error = Some(format!("EBADF:{message}")),
            },
            _ => {
                upload.error = Some("EINVAL:unsupported transmission medium".into());
            }
        }
        upload.finish(ctx)
    }
}

impl Upload {
    fn new(cmd: Command) -> Self {
        Self {
            cmd,
            base64: Base64::new(),
            data: Vec::new(),
            error: None,
        }
    }

    fn append(&mut self, chunk: &[u8]) {
        if self.error.is_some() {
            return;
        }
        if let Err(error) = self.base64.feed(chunk, &mut self.data) {
            self.error = Some(format!("EINVAL:{error}"));
        } else if self.data.len() > MAX_DATA {
            self.error = Some("EFBIG:image data too large".into());
            self.data = Vec::new();
        }
    }

    fn finish(mut self, ctx: &mut Context<'_>) -> Outcome {
        if self.cmd.medium == b'd' && self.error.is_none() {
            if let Err(error) = self.base64.finish(&mut self.data) {
                self.error = Some(format!("EINVAL:{error}"));
            }
        }
        let cmd = &self.cmd;
        if cmd.has(b'i') && cmd.has(b'I') && cmd.id != 0 && cmd.number != 0 {
            return reply(cmd, Err(("EINVAL", "both i and I given".into())), None);
        }
        if let Some(error) = self.error.take() {
            let (code, message) = error.split_once(':').unwrap_or(("EINVAL", &error));
            let code = match code {
                "EBADF" => "EBADF",
                "EFBIG" => "EFBIG",
                _ => "EINVAL",
            };
            return reply(cmd, Err((code, message.to_owned())), None);
        }
        let decoded = decode_image(cmd, std::mem::take(&mut self.data));
        let rgba = match decoded {
            Ok(rgba) => rgba,
            Err(failure) => return reply(cmd, Err(failure), None),
        };
        if cmd.action == b'q' {
            return reply(cmd, Ok(()), None);
        }
        let id = if cmd.id != 0 {
            cmd.id
        } else {
            ctx.store.unused_id()
        };
        let key = match ctx
            .store
            .insert(id, cmd.number, rgba.width, rgba.height, rgba.pixels)
        {
            Some(key) => key,
            None => return reply(cmd, Err(("EFBIG", "image too large".into())), None),
        };
        let advance = (cmd.action == b'T').then(|| place(cmd, key, ctx)).flatten();
        reply(cmd, Ok(()), Some(id)).with_advance(advance)
    }
}

impl Outcome {
    fn with_advance(mut self, advance: Option<(u32, u32)>) -> Self {
        self.advance = advance;
        self
    }
}

/// Decompresses and decodes the transmitted bytes.
fn decode_image(cmd: &Command, data: Vec<u8>) -> Result<decode::Rgba, Failure> {
    let data = match cmd.compression {
        0 => data,
        b'z' => decode::zlib(&data, MAX_DATA).map_err(|e| ("EINVAL", e.to_string()))?,
        _ => return Err(("EINVAL", "unsupported compression".into())),
    };
    let result = match cmd.format {
        24 => decode::raw(&data, cmd.data_width, cmd.data_height, 3),
        32 => decode::raw(&data, cmd.data_width, cmd.data_height, 4),
        100 => decode::png(&data),
        _ => return Err(("EINVAL", "unsupported format".into())),
    };
    result.map_err(|error| {
        let code = match error {
            DecodeError::NotEnoughData => "ENODATA",
            DecodeError::Png(_) => "EBADPNG",
            DecodeError::TooLarge => "EFBIG",
            _ => "EINVAL",
        };
        (code, error.to_string())
    })
}

/// The image a placement command refers to (by `i`, else by `I`).
fn find(cmd: &Command, store: &ImageStore) -> Result<u64, Failure> {
    let image = if cmd.id != 0 {
        store.by_id(cmd.id)
    } else if cmd.number != 0 {
        store.by_number(cmd.number)
    } else {
        return Err(("EINVAL", "no image id or number".into()));
    };
    image
        .map(|i| i.key)
        .ok_or_else(|| ("ENOENT", "no such image".into()))
}

/// Places image `key` at the cursor. Returns the cursor advance unless
/// the command keeps the cursor still or the placement is virtual.
fn place(cmd: &Command, key: u64, ctx: &mut Context<'_>) -> Option<(u32, u32)> {
    if cmd.unicode {
        // TODO: unicode placeholder placements are virtual; not displayed.
        return None;
    }
    let image = ctx.store.image(key)?;
    let (iw, ih) = (image.width, image.height);
    let x = cmd.x.min(iw);
    let y = cmd.y.min(ih);
    let src = SrcRect {
        x,
        y,
        width: if cmd.w == 0 {
            iw - x
        } else {
            cmd.w.min(iw - x)
        },
        height: if cmd.h == 0 {
            ih - y
        } else {
            cmd.h.min(ih - y)
        },
    };
    let placement = Placement {
        image: key,
        id: cmd.placement,
        row: ctx.cursor.1,
        col: ctx.cursor.0,
        offset_x: cmd.offset_x.min(ctx.cell.width.saturating_sub(1)),
        offset_y: cmd.offset_y.min(ctx.cell.height.saturating_sub(1)),
        src,
        cols: cmd.cols,
        rows: cmd.rows,
        z: cmd.z,
        sixel: false,
    };
    ctx.store.place(placement);
    (!cmd.cursor_fixed).then(|| placement.span(ctx.cell))
}

/// Runs a delete command; lowercase keeps image data, uppercase frees
/// images left without placements.
fn delete(cmd: &Command, ctx: &mut Context<'_>) {
    let free = cmd.delete.is_ascii_uppercase();
    let cell = ctx.cell;
    let (cur_col, cur_row) = ctx.cursor;
    // `x`/`y` are 1-based cell coordinates.
    let col = cmd.x.saturating_sub(1);
    let row = i32::try_from(cmd.y.saturating_sub(1)).unwrap_or(i32::MAX);
    let store = &mut *ctx.store;
    let keys_of = |store: &ImageStore, ids: &mut dyn Iterator<Item = u32>| -> Vec<u64> {
        ids.filter_map(|id| store.by_id(id).map(|i| i.key))
            .collect()
    };
    match cmd.delete.to_ascii_lowercase() {
        b'a' => store.remove_placements(free, |_| true),
        b'i' | b'n' | b'r' => {
            let keys: Vec<u64> = match cmd.delete.to_ascii_lowercase() {
                b'i' => keys_of(store, &mut std::iter::once(cmd.id)),
                b'n' => store
                    .by_number(cmd.number)
                    .map(|i| i.key)
                    .into_iter()
                    .collect(),
                _ => {
                    let ids: Vec<u32> = store_ids(store, cmd.x, cmd.y);
                    keys_of(store, &mut ids.into_iter())
                }
            };
            let only = (!cmd.delete.eq_ignore_ascii_case(&b'r') && cmd.placement != 0)
                .then_some(cmd.placement);
            store.remove_placements(free, |p| {
                keys.contains(&p.image) && only.is_none_or(|id| p.id == id)
            });
            if free && only.is_none() {
                for key in keys {
                    store.remove_image(key);
                }
            }
        }
        b'c' => store.remove_placements(free, |p| p.covers(cur_col, cur_row, cell)),
        b'p' => store.remove_placements(free, |p| p.covers(col, row, cell)),
        b'q' => store.remove_placements(free, |p| p.covers(col, row, cell) && p.z == cmd.z),
        b'x' => store.remove_placements(free, |p| {
            (0..p.span(cell).1).any(|r| p.covers(col, p.row.saturating_add(r as i32), cell))
        }),
        b'y' => store.remove_placements(free, |p| {
            (0..p.span(cell).0).any(|c| p.covers(p.col.saturating_add(c), row, cell))
        }),
        b'z' => store.remove_placements(free, |p| p.z == cmd.z),
        _ => {} // Frames (`f`) and unknown targets.
    }
}

/// Client ids of stored images within `lo..=hi`.
fn store_ids(store: &ImageStore, lo: u32, hi: u32) -> Vec<u32> {
    store
        .images()
        .map(|i| i.id)
        .filter(|id| (lo..=hi).contains(id))
        .collect()
}

/// The `i` value of a control string that failed to parse, or 0.
fn lenient_id(body: &[u8]) -> u32 {
    let control = body.split(|&b| b == b';').next().unwrap_or_default();
    control
        .split(|&b| b == b',')
        .find_map(|pair| pair.strip_prefix(b"i="))
        .and_then(|v| std::str::from_utf8(v).ok()?.parse().ok())
        .unwrap_or(0)
}

/// Frames a reply honoring `q`. Nothing is sent when the client named no
/// image (neither `i` nor `I`). `id` overrides the id echoed back (for
/// images numbered with `I`).
fn reply(cmd: &Command, result: Result<(), Failure>, id: Option<u32>) -> Outcome {
    let silent = match &result {
        Ok(()) => cmd.quiet >= 1,
        Err(_) => cmd.quiet >= 2,
    };
    if silent || (cmd.id == 0 && cmd.number == 0) {
        return Outcome::default();
    }
    let mut out = format!("\x1b_Gi={}", id.unwrap_or(cmd.id));
    if cmd.number != 0 {
        out.push_str(&format!(",I={}", cmd.number));
    }
    if cmd.placement != 0 {
        out.push_str(&format!(",p={}", cmd.placement));
    }
    out.push(';');
    match result {
        Ok(()) => out.push_str("OK"),
        Err((code, message)) => {
            // Keep the reply a single clean line.
            let message: String = message.chars().filter(|c| !c.is_control()).collect();
            out.push_str(&format!("{code}:{message}"));
        }
    }
    out.push_str("\x1b\\");
    Outcome {
        reply: Some(out.into_bytes()),
        advance: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::decode::tests::{encode_base64, encode_png, encode_zlib};

    const CELL: CellPixels = CellPixels {
        width: 10,
        height: 20,
    };

    struct Fixture {
        graphics: Graphics,
        store: ImageStore,
        cursor: (u32, i32),
    }

    impl Fixture {
        fn new() -> Self {
            Self {
                graphics: Graphics::new(),
                store: ImageStore::default(),
                cursor: (0, 0),
            }
        }

        fn run(&mut self, body: &str) -> Outcome {
            self.run_bytes(body.as_bytes())
        }

        fn run_bytes(&mut self, body: &[u8]) -> Outcome {
            let mut ctx = Context {
                store: &mut self.store,
                cursor: self.cursor,
                cell: CELL,
            };
            self.graphics.handle(body, &mut ctx)
        }

        fn reply(&mut self, body: &str) -> String {
            let outcome = self.run(body);
            String::from_utf8(outcome.reply.unwrap_or_default()).unwrap()
        }
    }

    /// Base64 of `n` opaque RGBA pixels of `color`.
    fn rgba(n: usize, color: [u8; 4]) -> String {
        encode_base64(&color.repeat(n))
    }

    #[test]
    fn transmit_rgba_replies_ok_and_stores() {
        let mut f = Fixture::new();
        let data = rgba(4, [1, 2, 3, 255]);
        let reply = f.reply(&format!("a=t,f=32,s=2,v=2,i=7;{data}"));
        assert_eq!(reply, "\x1b_Gi=7;OK\x1b\\");
        let image = f.store.by_id(7).unwrap();
        assert_eq!((image.width, image.height), (2, 2));
        assert_eq!(image.pixel(1, 1), [1, 2, 3, 255]);
        assert!(f.store.placements().is_empty(), "a=t does not display");
    }

    #[test]
    fn transmit_rgb_and_zlib() {
        let mut f = Fixture::new();
        let packed = encode_zlib(&[9, 8, 7]);
        let reply = f.reply(&format!("f=24,s=1,v=1,o=z,i=3;{}", encode_base64(&packed)));
        assert_eq!(reply, "\x1b_Gi=3;OK\x1b\\");
        assert_eq!(f.store.by_id(3).unwrap().pixel(0, 0), [9, 8, 7, 255]);
    }

    #[test]
    fn transmit_png_and_display_moves_cursor() {
        let mut f = Fixture::new();
        f.cursor = (2, 1);
        let png = encode_png(25, 30, &[200; 25 * 30 * 4]);
        let outcome = f.run(&format!("a=T,f=100,i=1;{}", encode_base64(&png)));
        assert_eq!(outcome.reply.unwrap(), b"\x1b_Gi=1;OK\x1b\\");
        assert_eq!(outcome.advance, Some((3, 2)));
        let p = f.store.placements()[0];
        assert_eq!((p.col, p.row), (2, 1));
        assert_eq!((p.src.width, p.src.height), (25, 30));
    }

    #[test]
    fn cursor_policy_one_keeps_cursor() {
        let mut f = Fixture::new();
        let outcome = f.run(&format!("a=T,s=1,v=1,C=1;{}", rgba(1, [0; 4])));
        assert_eq!(outcome.advance, None);
        assert_eq!(f.store.placements().len(), 1);
    }

    #[test]
    fn chunked_transmission_assembles_payload() {
        let mut f = Fixture::new();
        let data = rgba(3, [5, 6, 7, 255]); // 16 chars
        let (a, b) = data.split_at(8);
        assert_eq!(
            f.run(&format!("a=T,s=3,v=1,i=4,m=1;{a}")),
            Outcome::default()
        );
        assert!(
            f.store.by_id(4).is_none(),
            "not stored before the last chunk"
        );
        let outcome = f.run(&format!("m=0;{b}"));
        assert_eq!(outcome.reply.unwrap(), b"\x1b_Gi=4;OK\x1b\\");
        assert_eq!(outcome.advance, Some((1, 1)));
        assert_eq!(f.store.by_id(4).unwrap().pixel(2, 0), [5, 6, 7, 255]);
    }

    #[test]
    fn other_commands_may_interleave_with_a_chunked_upload() {
        let mut f = Fixture::new();
        f.run(&format!("i=1,s=1,v=1;{}", rgba(1, [1; 4])));
        let data = rgba(1, [2; 4]);
        f.run(&format!("i=2,s=1,v=1,m=1;{}", &data[..4]));
        assert_eq!(f.reply("a=p,i=1"), "\x1b_Gi=1;OK\x1b\\");
        assert_eq!(
            f.reply(&format!("m=0;{}", &data[4..])),
            "\x1b_Gi=2;OK\x1b\\"
        );
    }

    #[test]
    fn quiet_modes_suppress_replies() {
        let mut f = Fixture::new();
        let ok = format!("i=1,s=1,v=1;{}", rgba(1, [0; 4]));
        assert_eq!(f.reply(&format!("q=1,{ok}")), "");
        assert_eq!(
            f.reply("q=1,a=p,i=99"),
            "\x1b_Gi=99;ENOENT:no such image\x1b\\"
        );
        assert_eq!(f.reply("q=2,a=p,i=99"), "");
    }

    #[test]
    fn no_reply_without_an_id() {
        let mut f = Fixture::new();
        assert_eq!(f.reply(&format!("a=T,s=1,v=1;{}", rgba(1, [0; 4]))), "");
        assert_eq!(f.store.len(), 1, "anonymous image still stored");
        assert_eq!(f.reply("a=p,i=0"), "");
    }

    #[test]
    fn image_number_gets_an_assigned_id() {
        let mut f = Fixture::new();
        let reply = f.reply(&format!("I=13,s=1,v=1;{}", rgba(1, [0; 4])));
        let id = f.store.by_number(13).unwrap().id;
        assert_ne!(id, 0);
        assert_eq!(reply, format!("\x1b_Gi={id},I=13;OK\x1b\\"));
        assert_eq!(
            f.reply("a=p,I=13,p=2"),
            format!("\x1b_Gi={id},I=13,p=2;OK\x1b\\")
        );
    }

    #[test]
    fn query_validates_without_storing() {
        let mut f = Fixture::new();
        assert_eq!(
            f.reply(&format!("a=q,i=31,s=1,v=1;{}", rgba(1, [0; 4]))),
            "\x1b_Gi=31;OK\x1b\\"
        );
        assert!(f.store.is_empty());
        assert_eq!(
            f.reply("a=q,i=31,s=2,v=2;AAAA"),
            "\x1b_Gi=31;ENODATA:insufficient image data\x1b\\"
        );
    }

    #[test]
    fn errors_are_reported_with_codes() {
        let mut f = Fixture::new();
        assert!(f.reply("i=1,f=100;AAAA").starts_with("\x1b_Gi=1;EBADPNG:"));
        assert!(f.reply("i=1,f=7;AAAA").starts_with("\x1b_Gi=1;EINVAL:"));
        assert!(f.reply("i=1;@@@@").starts_with("\x1b_Gi=1;EINVAL:"));
        assert!(f.reply("i=1,s=0,v=0;AAAA").starts_with("\x1b_Gi=1;EINVAL:"));
        assert!(f.reply("i=5,t=s;AAAA").starts_with("\x1b_Gi=5;EINVAL:"));
        assert!(f.reply("i=5,a=f").starts_with("\x1b_Gi=5;EINVAL:"));
        assert!(f.reply("i=9,x=oops").starts_with("\x1b_Gi=9;EINVAL:"));
        assert!(
            f.reply("i=1,I=2,s=1,v=1;AAAAAA==")
                .starts_with("\x1b_Gi=1,I=2;EINVAL:")
        );
    }

    #[test]
    fn place_uses_source_rect_size_and_offsets() {
        let mut f = Fixture::new();
        f.run(&format!("i=1,s=4,v=4;{}", rgba(16, [0; 4])));
        f.cursor = (1, 2);
        let outcome = f.run("a=p,i=1,p=3,x=1,y=1,w=10,h=2,X=99,Y=4,c=2,z=-1");
        assert_eq!(outcome.reply.unwrap(), b"\x1b_Gi=1,p=3;OK\x1b\\");
        let p = f.store.placements()[0];
        assert_eq!(
            p.src,
            SrcRect {
                x: 1,
                y: 1,
                width: 3,
                height: 2
            }
        );
        assert_eq!(
            (p.offset_x, p.offset_y),
            (9, 4),
            "X clamped inside the cell"
        );
        assert_eq!((p.cols, p.z, p.id), (2, -1, 3));
    }

    fn two_images_placed(f: &mut Fixture) {
        f.run(&format!("i=1,s=1,v=1;{}", rgba(1, [0; 4])));
        f.run(&format!("i=2,s=1,v=1;{}", rgba(1, [0; 4])));
        f.cursor = (0, 0);
        f.run("a=p,i=1,p=1");
        f.cursor = (5, 3);
        f.run("a=p,i=2,z=4");
    }

    #[test]
    fn delete_all_keeps_or_frees_data() {
        let mut f = Fixture::new();
        two_images_placed(&mut f);
        assert_eq!(f.run("a=d"), Outcome::default());
        assert!(f.store.placements().is_empty());
        assert_eq!(f.store.len(), 2);
        two_images_placed(&mut f);
        f.run("a=d,d=A");
        assert!(f.store.is_empty());
    }

    #[test]
    fn delete_by_id_placement_and_cell() {
        let mut f = Fixture::new();
        two_images_placed(&mut f);
        f.run("a=d,d=i,i=1,p=7");
        assert_eq!(f.store.placements().len(), 2, "wrong placement id");
        f.run("a=d,d=I,i=1");
        assert!(f.store.by_id(1).is_none());
        assert_eq!(f.store.placements().len(), 1);

        two_images_placed(&mut f);
        f.run("a=d,d=p,x=6,y=4");
        assert!(f.store.placements().iter().all(|p| p.col == 0));
        f.cursor = (0, 0);
        f.run("a=d,d=c");
        assert!(f.store.placements().is_empty());
    }

    #[test]
    fn delete_by_z_row_column_and_range() {
        let mut f = Fixture::new();
        two_images_placed(&mut f);
        f.run("a=d,d=z,z=4");
        assert_eq!(f.store.placements().len(), 1);
        two_images_placed(&mut f);
        f.run("a=d,d=y,y=4");
        assert!(f.store.placements().iter().all(|p| p.row == 0));
        f.run("a=d,d=x,x=1");
        assert!(f.store.placements().is_empty());
        two_images_placed(&mut f);
        f.run("a=d,d=q,x=6,y=4,z=0");
        assert_eq!(f.store.placements().len(), 2, "z mismatch");
        f.run("a=d,d=R,x=1,y=2");
        assert!(f.store.is_empty());
    }

    #[test]
    fn file_transmission_reads_regular_files() {
        let dir = std::env::temp_dir().join(format!("nxg-kitty-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("img.rgba");
        std::fs::write(&path, [1, 2, 3, 4, 5, 6, 7, 8]).unwrap();
        let encoded = encode_base64(path.to_string_lossy().as_bytes());
        let mut f = Fixture::new();
        assert_eq!(
            f.reply(&format!("i=1,t=f,s=1,v=1,O=4;{encoded}")),
            "\x1b_Gi=1;OK\x1b\\"
        );
        assert_eq!(f.store.by_id(1).unwrap().pixel(0, 0), [5, 6, 7, 8]);
        assert!(path.exists(), "t=f never deletes");

        let dir_reply = f.reply(&format!(
            "i=2,t=f,s=1,v=1;{}",
            encode_base64(dir.to_string_lossy().as_bytes())
        ));
        assert!(dir_reply.starts_with("\x1b_Gi=2;EBADF:"), "{dir_reply:?}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn temp_file_transmission_requires_marker_and_deletes() {
        let dir = std::env::temp_dir();
        let name = format!("tty-graphics-protocol-nxg-{}", std::process::id());
        let path = dir.join(&name);
        std::fs::write(&path, [1, 2, 3, 4]).unwrap();
        let mut f = Fixture::new();
        let encoded = encode_base64(path.to_string_lossy().as_bytes());
        assert_eq!(
            f.reply(&format!("i=1,t=t,s=1,v=1;{encoded}")),
            "\x1b_Gi=1;OK\x1b\\"
        );
        assert!(!path.exists(), "temp file is deleted after reading");

        let plain = dir.join(format!("nxg-plain-{}", std::process::id()));
        std::fs::write(&plain, [1, 2, 3, 4]).unwrap();
        let encoded = encode_base64(plain.to_string_lossy().as_bytes());
        assert!(
            f.reply(&format!("i=2,t=t,s=1,v=1;{encoded}"))
                .contains("EBADF")
        );
        assert!(plain.exists(), "files without the marker are left alone");
        std::fs::remove_file(&plain).unwrap();
    }

    #[test]
    fn unicode_placeholder_is_stored_but_not_displayed() {
        let mut f = Fixture::new();
        let outcome = f.run(&format!("a=T,U=1,i=1,s=1,v=1;{}", rgba(1, [0; 4])));
        assert_eq!(outcome.advance, None);
        assert!(f.store.placements().is_empty());
        assert!(f.store.by_id(1).is_some());
    }

    #[test]
    fn oversized_chunked_upload_fails_cleanly() {
        let mut f = Fixture::new();
        let chunk = "A".repeat(4096);
        f.run(&format!("i=1,s=1,v=1,m=1;{chunk}"));
        let mut more = 0;
        while more * 3072 < MAX_DATA + 4096 {
            f.run_bytes(format!("m=1;{chunk}").as_bytes());
            more += 1;
        }
        let reply = f.reply("m=0;");
        assert!(reply.starts_with("\x1b_Gi=1;EFBIG:"), "{reply:?}");
        assert!(f.store.is_empty());
    }
}
