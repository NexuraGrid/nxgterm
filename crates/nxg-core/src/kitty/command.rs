//! Kitty graphics command parsing: `key=value,...;payload`.

/// One parsed graphics command (the APC payload after the leading `G`).
/// Fields hold the protocol defaults when a key is absent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    /// `a`: t, T, p, d, q (f, a, c are animation, unsupported).
    pub action: u8,
    /// `q`: 1 hides OK replies, 2 hides errors too.
    pub quiet: u32,
    /// `f`: 24, 32 or 100 (PNG).
    pub format: u32,
    /// `t`: d (direct), f (file), t (temp file), s (shared memory).
    pub medium: u8,
    /// `o`: b'z' for zlib.
    pub compression: u8,
    /// `s`, `v`: pixel size of raw data.
    pub data_width: u32,
    pub data_height: u32,
    /// `S`, `O`: bytes to read from a file and where to start.
    pub data_size: u64,
    pub data_offset: u64,
    /// `i`, `I`, `p`.
    pub id: u32,
    pub number: u32,
    pub placement: u32,
    /// `m`: more chunks follow.
    pub more: bool,
    /// `x`, `y`, `w`, `h`: source rectangle.
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    /// `X`, `Y`: pixel offset inside the first cell.
    pub offset_x: u32,
    pub offset_y: u32,
    /// `c`, `r`: display size in cells.
    pub cols: u32,
    pub rows: u32,
    /// `z`.
    pub z: i32,
    /// `C`: 1 keeps the cursor where it is.
    pub cursor_fixed: bool,
    /// `U`: unicode placeholder (virtual) placement.
    pub unicode: bool,
    /// `d`: what to delete.
    pub delete: u8,
    /// Keys present, for telling defaults from explicit values.
    pub keys: Vec<u8>,
    /// Bytes after `;` (base64 data or path), possibly empty.
    pub payload: Vec<u8>,
}

impl Default for Command {
    fn default() -> Self {
        Self {
            action: b't',
            quiet: 0,
            format: 32,
            medium: b'd',
            compression: 0,
            data_width: 0,
            data_height: 0,
            data_size: 0,
            data_offset: 0,
            id: 0,
            number: 0,
            placement: 0,
            more: false,
            x: 0,
            y: 0,
            w: 0,
            h: 0,
            offset_x: 0,
            offset_y: 0,
            cols: 0,
            rows: 0,
            z: 0,
            cursor_fixed: false,
            unicode: false,
            delete: b'a',
            keys: Vec::new(),
            payload: Vec::new(),
        }
    }
}

impl Command {
    /// Parses `body` (without the leading `G`). Unknown keys are ignored;
    /// malformed values are an error naming the key.
    pub fn parse(body: &[u8]) -> Result<Self, String> {
        let (control, payload) = match body.iter().position(|&b| b == b';') {
            Some(i) => (&body[..i], &body[i + 1..]),
            None => (body, &[][..]),
        };
        let mut cmd = Self {
            payload: payload.to_vec(),
            ..Self::default()
        };
        for pair in control.split(|&b| b == b',').filter(|p| !p.is_empty()) {
            let [key, b'=', value @ ..] = pair else {
                return Err(format!("malformed key {:?}", String::from_utf8_lossy(pair)));
            };
            let bad = || format!("invalid value for key {}", *key as char);
            let uint = || -> Result<u32, String> {
                std::str::from_utf8(value)
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .ok_or_else(bad)
            };
            let ch = || -> Result<u8, String> {
                match value {
                    [c] => Ok(*c),
                    _ => Err(bad()),
                }
            };
            match key {
                b'a' => cmd.action = ch()?,
                b'q' => cmd.quiet = uint()?,
                b'f' => cmd.format = uint()?,
                b't' => cmd.medium = ch()?,
                b'o' => cmd.compression = ch()?,
                b's' => cmd.data_width = uint()?,
                b'v' => cmd.data_height = uint()?,
                b'S' => cmd.data_size = u64::from(uint()?),
                b'O' => cmd.data_offset = u64::from(uint()?),
                b'i' => cmd.id = uint()?,
                b'I' => cmd.number = uint()?,
                b'p' => cmd.placement = uint()?,
                b'm' => cmd.more = uint()? == 1,
                b'x' => cmd.x = uint()?,
                b'y' => cmd.y = uint()?,
                b'w' => cmd.w = uint()?,
                b'h' => cmd.h = uint()?,
                b'X' => cmd.offset_x = uint()?,
                b'Y' => cmd.offset_y = uint()?,
                b'c' => cmd.cols = uint()?,
                b'r' => cmd.rows = uint()?,
                b'z' => {
                    cmd.z = std::str::from_utf8(value)
                        .ok()
                        .and_then(|v| v.parse().ok())
                        .ok_or_else(bad)?;
                }
                b'C' => cmd.cursor_fixed = uint()? == 1,
                b'U' => cmd.unicode = uint()? == 1,
                b'd' => cmd.delete = ch()?,
                _ => continue,
            }
            cmd.keys.push(*key);
        }
        Ok(cmd)
    }

    pub fn has(&self, key: u8) -> bool {
        self.keys.contains(&key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_command_has_defaults() {
        let cmd = Command::parse(b"").unwrap();
        assert_eq!(cmd, Command::default());
        assert_eq!((cmd.action, cmd.format, cmd.medium), (b't', 32, b'd'));
    }

    #[test]
    fn parses_keys_and_payload() {
        let cmd = Command::parse(b"a=T,f=100,i=31,p=2,q=2,m=1,o=z,C=1,z=-5;QUJD").unwrap();
        assert_eq!(cmd.action, b'T');
        assert_eq!(cmd.format, 100);
        assert_eq!((cmd.id, cmd.placement, cmd.quiet), (31, 2, 2));
        assert!(cmd.more && cmd.cursor_fixed);
        assert_eq!(cmd.compression, b'z');
        assert_eq!(cmd.z, -5);
        assert_eq!(cmd.payload, b"QUJD");
        assert!(cmd.has(b'i') && !cmd.has(b'I'));
    }

    #[test]
    fn parses_geometry_keys() {
        let cmd =
            Command::parse(b"s=10,v=20,x=1,y=2,w=3,h=4,X=5,Y=6,c=7,r=8,S=9,O=10,I=11").unwrap();
        assert_eq!((cmd.data_width, cmd.data_height), (10, 20));
        assert_eq!((cmd.x, cmd.y, cmd.w, cmd.h), (1, 2, 3, 4));
        assert_eq!(
            (cmd.offset_x, cmd.offset_y, cmd.cols, cmd.rows),
            (5, 6, 7, 8)
        );
        assert_eq!((cmd.data_size, cmd.data_offset, cmd.number), (9, 10, 11));
    }

    #[test]
    fn payload_may_contain_separators() {
        let cmd = Command::parse(b"a=q;a=b,c;d").unwrap();
        assert_eq!(cmd.payload, b"a=b,c;d");
    }

    #[test]
    fn ignores_unknown_keys_and_empty_pairs() {
        let cmd = Command::parse(b"a=p,,K=9,i=1").unwrap();
        assert_eq!((cmd.action, cmd.id), (b'p', 1));
    }

    #[test]
    fn rejects_malformed_values() {
        assert!(Command::parse(b"i=abc").is_err());
        assert!(Command::parse(b"i=-1").is_err());
        assert!(Command::parse(b"a=TT").is_err());
        assert!(Command::parse(b"i").is_err());
        assert!(Command::parse(b"i=99999999999").is_err());
    }
}
