use std::cmp;

#[derive(Clone, Copy, Default, Debug)]
pub struct Cell {
    pub ch: char,
    pub fg: u8,
    pub bg: u8,
    pub bold: bool,
}

enum EscapeState {
    Normal,
    Esc,
    Csi,
}

pub struct VtScreen {
    rows: usize,
    cols: usize,
    cells: Vec<Vec<Cell>>,
    cursor_row: usize,
    cursor_col: usize,
    fg: u8,
    bg: u8,
    bold: bool,
    state: EscapeState,
    param_buf: String,
}

impl VtScreen {
    pub fn new(rows: usize, cols: usize) -> Self {
        let cells = vec![vec![Cell::default(); cols]; rows];
        Self {
            rows,
            cols,
            cells,
            cursor_row: 0,
            cursor_col: 0,
            fg: 7,
            bg: 0,
            bold: false,
            state: EscapeState::Normal,
            param_buf: String::new(),
        }
    }

    pub fn resize(&mut self, rows: usize, cols: usize) {
        let mut new = vec![vec![Cell::default(); cols]; rows];
        for r in 0..self.rows.min(rows) {
            for c in 0..self.cols.min(cols) {
                new[r][c] = self.cells[r][c];
            }
        }
        self.cells = new;
        self.rows = rows;
        self.cols = cols;
        self.cursor_row = self.cursor_row.min(rows.saturating_sub(1));
        self.cursor_col = self.cursor_col.min(cols.saturating_sub(1));
    }

    pub fn feed(&mut self, byte: u8) {
        match self.state {
            EscapeState::Normal => self.normal(byte),
            EscapeState::Esc => self.esc(byte),
            EscapeState::Csi => self.csi(byte),
        }
    }

    fn normal(&mut self, byte: u8) {
        match byte {
            0x1b => {
                self.state = EscapeState::Esc;
            }
            b'\r' => self.cursor_col = 0,
            b'\n' => {
                self.cursor_row += 1;
                if self.cursor_row >= self.rows {
                    self.scroll_up();
                    self.cursor_row = self.rows.saturating_sub(1);
                }
            }
            b'\t' => {
                let next = ((self.cursor_col / 8) + 1) * 8;
                self.cursor_col = next.min(self.cols.saturating_sub(1));
            }
            0x08 => {
                if self.cursor_col > 0 {
                    self.cursor_col -= 1;
                }
            }
            0x07 | 0x00..=0x06 | 0x0E..=0x1A | 0x1C..=0x1F => {}
            _ => {
                if let Some(c) = char::from_u32(byte as u32) {
                    self.put(c);
                }
            }
        }
    }

    fn esc(&mut self, byte: u8) {
        match byte {
            b'[' => {
                self.state = EscapeState::Csi;
                self.param_buf.clear();
            }
            _ => self.state = EscapeState::Normal,
        }
    }

    fn csi(&mut self, byte: u8) {
        if (b'0'..=b'9').contains(&byte) || byte == b';' {
            self.param_buf.push(byte as char);
        } else if byte == b'?' || byte == b'>' || byte == b'!' {
        } else {
            let params: Vec<u16> = if self.param_buf.is_empty() {
                vec![]
            } else {
                self.param_buf
                    .split(';')
                    .filter_map(|s| s.parse().ok())
                    .collect()
            };
            self.dispatch_csi(byte, &params);
            self.state = EscapeState::Normal;
        }
    }

    fn dispatch_csi(&mut self, action: u8, params: &[u16]) {
        let p = |idx: usize, default: u16| -> usize {
            params.get(idx).copied().unwrap_or(default).max(1) as usize
        };
        match action {
            b'A' => {
                let n = p(0, 1);
                self.cursor_row = self.cursor_row.saturating_sub(n);
            }
            b'B' => {
                let n = p(0, 1);
                self.cursor_row = cmp::min(self.cursor_row + n, self.rows.saturating_sub(1));
            }
            b'C' => {
                let n = p(0, 1);
                self.cursor_col = cmp::min(self.cursor_col + n, self.cols.saturating_sub(1));
            }
            b'D' => {
                let n = p(0, 1);
                self.cursor_col = self.cursor_col.saturating_sub(n);
            }
            b'H' | b'f' => {
                let row = p(0, 1);
                let col = p(1, 1);
                self.cursor_row = cmp::min(row.saturating_sub(1), self.rows.saturating_sub(1));
                self.cursor_col = cmp::min(col.saturating_sub(1), self.cols.saturating_sub(1));
            }
            b'J' => {
                let mode = params.first().copied().unwrap_or(0);
                match mode {
                    0 => {
                        for c in self.cursor_col..self.cols {
                            self.cells[self.cursor_row][c] = Cell::default();
                        }
                        for r in self.cursor_row + 1..self.rows {
                            for c in 0..self.cols {
                                self.cells[r][c] = Cell::default();
                            }
                        }
                    }
                    2 => {
                        for r in 0..self.rows {
                            for c in 0..self.cols {
                                self.cells[r][c] = Cell::default();
                            }
                        }
                        self.cursor_row = 0;
                        self.cursor_col = 0;
                    }
                    _ => {}
                }
            }
            b'K' => {
                let mode = params.first().copied().unwrap_or(0);
                match mode {
                    0 => {
                        for c in self.cursor_col..self.cols {
                            self.cells[self.cursor_row][c] = Cell::default();
                        }
                    }
                    2 => {
                        for c in 0..self.cols {
                            self.cells[self.cursor_row][c] = Cell::default();
                        }
                    }
                    _ => {}
                }
            }
            b'm' => {
                if params.is_empty() {
                    self.fg = 7;
                    self.bg = 0;
                    self.bold = false;
                } else {
                    for &param in params {
                        match param {
                            0 => {
                                self.fg = 7;
                                self.bg = 0;
                                self.bold = false;
                            }
                            1 => self.bold = true,
                            22 => self.bold = false,
                            30..=37 => self.fg = (param - 30) as u8,
                            40..=47 => self.bg = (param - 40) as u8,
                            90..=97 => self.fg = (param - 90 + 8) as u8,
                            100..=107 => self.bg = (param - 100 + 8) as u8,
                            39 => self.fg = 7,
                            49 => self.bg = 0,
                            _ => {}
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn put(&mut self, c: char) {
        if self.cursor_row >= self.rows || self.cursor_col >= self.cols {
            return;
        }
        self.cells[self.cursor_row][self.cursor_col] = Cell {
            ch: c,
            fg: self.fg,
            bg: self.bg,
            bold: self.bold,
        };
        self.cursor_col += 1;
        if self.cursor_col >= self.cols {
            self.cursor_col = 0;
            self.cursor_row += 1;
            if self.cursor_row >= self.rows {
                self.scroll_up();
                self.cursor_row = self.rows.saturating_sub(1);
            }
        }
    }

    fn scroll_up(&mut self) {
        for r in 1..self.rows {
            self.cells.swap(r - 1, r);
        }
        let last = self.rows.saturating_sub(1);
        self.cells[last] = vec![Cell::default(); self.cols];
    }

    pub fn cell(&self, row: usize, col: usize) -> Cell {
        self.cells
            .get(row)
            .and_then(|r| r.get(col))
            .copied()
            .unwrap_or_default()
    }

    pub fn rows(&self) -> usize {
        self.rows
    }
    pub fn cols(&self) -> usize {
        self.cols
    }
}
