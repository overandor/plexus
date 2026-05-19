use crate::vt::VtScreen;
use libc::{c_ulong, ioctl, winsize, TIOCSWINSZ};
use nix::pty::{openpty, OpenptyResult, Winsize as NixWinsize};
use nix::unistd::{close, dup2, fork, setsid, ForkResult};
use std::ffi::CString;
use std::os::fd::{AsRawFd, OwnedFd};

#[derive(Clone, Copy, Debug)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
}

impl Rect {
    pub fn contains(&self, x: u16, y: u16) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }

    pub fn center(&self) -> (u16, u16) {
        (self.x + self.w / 2, self.y + self.h / 2)
    }
}

pub struct Pane {
    pub id: usize,
    pub screen: VtScreen,
    pub master: OwnedFd,
    pub pid: nix::unistd::Pid,
    pub rect: Rect,
}

impl Pane {
    pub fn spawn(id: usize, rect: Rect) -> nix::Result<Self> {
        let rows = rect.h.max(1);
        let cols = rect.w.max(1);
        let ws = NixWinsize {
            ws_row: rows,
            ws_col: cols,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        let OpenptyResult { master, slave } = openpty(Some(&ws), None)?;

        match unsafe { fork()? } {
            ForkResult::Child => {
                drop(master);
                let slave_fd = slave.as_raw_fd();
                setsid()?;
                #[cfg(target_os = "macos")]
                unsafe {
                    ioctl(slave_fd, libc::TIOCSCTTY as c_ulong, 0);
                }
                dup2(slave_fd, 0)?;
                dup2(slave_fd, 1)?;
                dup2(slave_fd, 2)?;
                close(slave_fd)?;
                drop(slave);

                let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into());
                let shell_c = CString::new(shell).map_err(|_| nix::Error::EINVAL)?;
                let arg0 = CString::new("-i").map_err(|_| nix::Error::EINVAL)?;
                nix::unistd::execvp(&shell_c, &[shell_c.clone(), arg0])?;
                std::process::exit(1);
            }
            ForkResult::Parent { child } => {
                drop(slave);
                let mut screen = VtScreen::new(rows as usize, cols as usize);
                screen.resize(rows as usize, cols as usize);
                Ok(Pane {
                    id,
                    screen,
                    master,
                    pid: child,
                    rect,
                })
            }
        }
    }

    pub fn resize(&mut self, rect: Rect) {
        let rows = rect.h.max(1);
        let cols = rect.w.max(1);
        self.screen.resize(rows as usize, cols as usize);
        let ws = winsize {
            ws_row: rows,
            ws_col: cols,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        unsafe {
            ioctl(self.master.as_raw_fd(), TIOCSWINSZ as c_ulong, &ws);
        }
        self.rect = rect;
    }

    pub fn read_pty(&mut self) {
        let mut buf = [0u8; 4096];
        loop {
            match nix::unistd::read(self.master.as_raw_fd(), &mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    for &b in &buf[..n] {
                        self.screen.feed(b);
                    }
                }
                Err(nix::errno::Errno::EAGAIN) => break,
                Err(_) => break,
            }
        }
    }

    pub fn write_pty(&mut self, data: &[u8]) {
        let _ = nix::unistd::write(&self.master, data);
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SplitDir {
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NavDir {
    Left,
    Right,
    Up,
    Down,
}

pub struct App {
    pub panes: Vec<Pane>,
    pub active: usize,
    pub next_id: usize,
    pub prefix: bool,
    pub should_quit: bool,
    pub status: String,
    pub term_w: u16,
    pub term_h: u16,
}

impl App {
    pub fn new(term_w: u16, term_h: u16) -> nix::Result<Self> {
        let status_h = 1u16;
        let pane_h = term_h.saturating_sub(status_h).max(1);
        let rect = Rect {
            x: 0,
            y: 0,
            w: term_w,
            h: pane_h,
        };
        let pane = Pane::spawn(0, rect)?;
        Ok(App {
            panes: vec![pane],
            active: 0,
            next_id: 1,
            prefix: false,
            should_quit: false,
            status: String::new(),
            term_w,
            term_h,
        })
    }

    pub fn set_status(&mut self, msg: impl Into<String>) {
        self.status = msg.into();
    }

    pub fn active_pane(&self) -> Option<&Pane> {
        self.panes.iter().find(|p| p.id == self.active)
    }

    pub fn active_pane_mut(&mut self) -> Option<&mut Pane> {
        self.panes.iter_mut().find(|p| p.id == self.active)
    }

    pub fn split(&mut self, dir: SplitDir) {
        let Some(pane) = self.active_pane() else { return };
        let old_rect = pane.rect;
        if old_rect.w < 4 || old_rect.h < 4 {
            self.set_status("pane too small to split");
            return;
        }

        let (rect_a, rect_b) = match dir {
            SplitDir::Vertical => {
                let half = old_rect.w / 2;
                (
                    Rect {
                        x: old_rect.x,
                        y: old_rect.y,
                        w: half,
                        h: old_rect.h,
                    },
                    Rect {
                        x: old_rect.x + half,
                        y: old_rect.y,
                        w: old_rect.w - half,
                        h: old_rect.h,
                    },
                )
            }
            SplitDir::Horizontal => {
                let half = old_rect.h / 2;
                (
                    Rect {
                        x: old_rect.x,
                        y: old_rect.y,
                        w: old_rect.w,
                        h: half,
                    },
                    Rect {
                        x: old_rect.x,
                        y: old_rect.y + half,
                        w: old_rect.w,
                        h: old_rect.h - half,
                    },
                )
            }
        };

        let old_id = self.active;
        if let Some(p) = self.panes.iter_mut().find(|p| p.id == old_id) {
            p.resize(rect_a);
        }

        let new_id = self.next_id;
        self.next_id += 1;
        match Pane::spawn(new_id, rect_b) {
            Ok(pane) => {
                self.panes.push(pane);
                self.active = new_id;
                self.set_status(format!("split {:?}", dir));
            }
            Err(e) => {
                self.set_status(format!("spawn failed: {}", e));
            }
        }
    }

    pub fn close_active(&mut self) {
        if self.panes.len() <= 1 {
            self.should_quit = true;
            return;
        }
        let removed = self.active;
        self.panes.retain(|p| p.id != removed);
        if let Some(first) = self.panes.first() {
            self.active = first.id;
        }
        self.redistribute();
        self.set_status("pane closed");
    }

    pub fn navigate(&mut self, dir: NavDir) {
        let Some(current) = self.active_pane() else { return };
        let (cx, cy) = current.rect.center();
        let mut best: Option<(usize, u32)> = None;
        for pane in &self.panes {
            if pane.id == self.active {
                continue;
            }
            let (px, py) = pane.rect.center();
            let (dx, dy) = (px as i32 - cx as i32, py as i32 - cy as i32);
            let aligned = match dir {
                NavDir::Left => dx < 0 && dy.abs() <= (current.rect.h + pane.rect.h) as i32 / 2,
                NavDir::Right => dx > 0 && dy.abs() <= (current.rect.h + pane.rect.h) as i32 / 2,
                NavDir::Up => dy < 0 && dx.abs() <= (current.rect.w + pane.rect.w) as i32 / 2,
                NavDir::Down => dy > 0 && dx.abs() <= (current.rect.w + pane.rect.w) as i32 / 2,
            };
            if aligned {
                let dist = (dx * dx + dy * dy) as u32;
                if best.map(|(_, d)| dist < d).unwrap_or(true) {
                    best = Some((pane.id, dist));
                }
            }
        }
        if let Some((id, _)) = best {
            self.active = id;
        }
    }

    pub fn resize_terminal(&mut self, w: u16, h: u16) {
        self.term_w = w;
        self.term_h = h;
        self.redistribute();
    }

    fn redistribute(&mut self) {
        if self.panes.is_empty() {
            return;
        }
        let status_h = 1u16;
        let pane_h = self.term_h.saturating_sub(status_h).max(1);
        let n = self.panes.len();
        let cols = ((n as f64).sqrt().ceil() as u16).max(1);
        let rows = ((n as u16 + cols - 1) / cols).max(1);
        let col_w = self.term_w / cols;
        let row_h = pane_h / rows;
        for (i, pane) in self.panes.iter_mut().enumerate() {
            let c = (i as u16) % cols;
            let r = (i as u16) / cols;
            let is_last_col = c == cols - 1;
            let is_last_row = r == rows - 1;
            let w = if is_last_col {
                self.term_w - c * col_w
            } else {
                col_w
            };
            let h = if is_last_row {
                pane_h - r * row_h
            } else {
                row_h
            };
            pane.resize(Rect {
                x: c * col_w,
                y: r * row_h,
                w: w.max(1),
                h: h.max(1),
            });
        }
    }
}
