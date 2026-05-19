mod app;
mod vt;

use app::{App, NavDir, SplitDir};
use crossterm::{
    cursor::MoveTo,
    event::{Event, KeyCode, KeyEvent, KeyModifiers},
    queue,
    style::{Color, Print, ResetColor, SetBackgroundColor, SetForegroundColor},
    terminal::{self, Clear, ClearType, DisableLineWrap, EnableLineWrap},
};
use nix::poll::{poll, PollFd, PollFlags};
use std::io::{stdout, Write};
use std::os::fd::AsFd;
use std::time::Duration;

fn ansi_to_crossterm(color: u8) -> Color {
    match color {
        0 => Color::Black,
        1 => Color::DarkRed,
        2 => Color::DarkGreen,
        3 => Color::DarkYellow,
        4 => Color::DarkBlue,
        5 => Color::DarkMagenta,
        6 => Color::DarkCyan,
        7 => Color::Grey,
        8 => Color::DarkGrey,
        9 => Color::Red,
        10 => Color::Green,
        11 => Color::Yellow,
        12 => Color::Blue,
        13 => Color::Magenta,
        14 => Color::Cyan,
        15 => Color::White,
        _ => Color::Reset,
    }
}

fn render(app: &App) -> std::io::Result<()> {
    let mut stdout = stdout();
    queue!(stdout, Clear(ClearType::All))?;

    for pane in &app.panes {
        let is_active = pane.id == app.active;
        let border_fg = if is_active {
            Color::Green
        } else {
            Color::DarkGrey
        };
        draw_rect(&mut stdout, &pane.rect, border_fg)?;

        let inner_w = pane.rect.w.saturating_sub(2).max(1) as usize;
        let inner_h = pane.rect.h.saturating_sub(2).max(1) as usize;
        let screen_rows = pane.screen.rows();
        let screen_cols = pane.screen.cols();

        for row in 0..inner_h {
            let screen_row = row.min(screen_rows.saturating_sub(1));
            let y = pane.rect.y + 1 + row as u16;
            if y >= app.term_h - 1 {
                break;
            }
            queue!(stdout, MoveTo(pane.rect.x + 1, y))?;
            for col in 0..inner_w {
                let screen_col = col.min(screen_cols.saturating_sub(1));
                let cell = pane.screen.cell(screen_row, screen_col);
                queue!(stdout, SetForegroundColor(ansi_to_crossterm(cell.fg)))?;
                queue!(stdout, SetBackgroundColor(ansi_to_crossterm(cell.bg)))?;
                queue!(stdout, Print(cell.ch))?;
            }
        }
    }

    let status_y = app.term_h.saturating_sub(1);
    queue!(stdout, MoveTo(0, status_y))?;
    queue!(stdout, SetBackgroundColor(Color::Blue))?;
    queue!(stdout, SetForegroundColor(Color::White))?;
    let info = format!(
        " plexus | pane {} | {} | Ctrl+a:prefix ",
        app.active, app.status
    );
    let pad_len = (app.term_w as usize).saturating_sub(info.len());
    let padded = format!("{}{}", info, " ".repeat(pad_len));
    let clipped: String = padded.chars().take(app.term_w as usize).collect();
    queue!(stdout, Print(clipped))?;
    queue!(stdout, ResetColor)?;

    stdout.flush()?;
    Ok(())
}

fn draw_rect(stdout: &mut std::io::Stdout, rect: &app::Rect, color: Color) -> std::io::Result<()> {
    if rect.w == 0 || rect.h == 0 {
        return Ok(());
    }
    queue!(stdout, SetForegroundColor(color))?;
    for x in rect.x..rect.x + rect.w {
        queue!(stdout, MoveTo(x, rect.y))?;
        queue!(stdout, Print('─'))?;
        queue!(stdout, MoveTo(x, rect.y + rect.h.saturating_sub(1)))?;
        queue!(stdout, Print('─'))?;
    }
    for y in rect.y..rect.y + rect.h {
        queue!(stdout, MoveTo(rect.x, y))?;
        queue!(stdout, Print('│'))?;
        queue!(stdout, MoveTo(rect.x + rect.w.saturating_sub(1), y))?;
        queue!(stdout, Print('│'))?;
    }
    queue!(stdout, MoveTo(rect.x, rect.y))?;
    queue!(stdout, Print('┌'))?;
    queue!(stdout, MoveTo(rect.x + rect.w.saturating_sub(1), rect.y))?;
    queue!(stdout, Print('┐'))?;
    queue!(stdout, MoveTo(rect.x, rect.y + rect.h.saturating_sub(1)))?;
    queue!(stdout, Print('└'))?;
    queue!(stdout, MoveTo(rect.x + rect.w.saturating_sub(1), rect.y + rect.h.saturating_sub(1)))?;
    queue!(stdout, Print('┘'))?;
    Ok(())
}

fn key_to_bytes(key: KeyEvent) -> Vec<u8> {
    let mut buf = Vec::new();
    match key.code {
        KeyCode::Char(c) => {
            if key.modifiers.contains(KeyModifiers::CONTROL) {
                if c.is_ascii_lowercase() {
                    buf.push((c as u8) & 0x1f);
                } else if c == ' ' {
                    buf.push(0);
                } else {
                    buf.push(c as u8);
                }
            } else {
                let ch = if key.modifiers.contains(KeyModifiers::SHIFT) {
                    c.to_ascii_uppercase()
                } else {
                    c
                };
                let mut tmp = [0u8; 4];
                ch.encode_utf8(&mut tmp);
                for &b in ch.encode_utf8(&mut tmp).as_bytes() {
                    buf.push(b);
                }
            }
        }
        KeyCode::Enter => buf.push(b'\r'),
        KeyCode::Tab => buf.push(b'\t'),
        KeyCode::Backspace => buf.push(0x7f),
        KeyCode::Esc => buf.push(0x1b),
        KeyCode::Up => buf.extend_from_slice(b"\x1b[A"),
        KeyCode::Down => buf.extend_from_slice(b"\x1b[B"),
        KeyCode::Right => buf.extend_from_slice(b"\x1b[C"),
        KeyCode::Left => buf.extend_from_slice(b"\x1b[D"),
        KeyCode::Home => buf.extend_from_slice(b"\x1b[H"),
        KeyCode::End => buf.extend_from_slice(b"\x1b[F"),
        KeyCode::PageUp => buf.extend_from_slice(b"\x1b[5~"),
        KeyCode::PageDown => buf.extend_from_slice(b"\x1b[6~"),
        KeyCode::Delete => buf.extend_from_slice(b"\x1b[3~"),
        KeyCode::Insert => buf.extend_from_slice(b"\x1b[2~"),
        KeyCode::F(1) => buf.extend_from_slice(b"\x1bOP"),
        KeyCode::F(2) => buf.extend_from_slice(b"\x1bOQ"),
        KeyCode::F(3) => buf.extend_from_slice(b"\x1bOR"),
        KeyCode::F(4) => buf.extend_from_slice(b"\x1bOS"),
        _ => {}
    }
    buf
}

fn handle_key(app: &mut App, key: KeyEvent) {
    if app.prefix {
        app.prefix = false;
        match key.code {
            KeyCode::Char('|') => app.split(SplitDir::Vertical),
            KeyCode::Char('-') => app.split(SplitDir::Horizontal),
            KeyCode::Char('c') => app.close_active(),
            KeyCode::Left => app.navigate(NavDir::Left),
            KeyCode::Right => app.navigate(NavDir::Right),
            KeyCode::Up => app.navigate(NavDir::Up),
            KeyCode::Down => app.navigate(NavDir::Down),
            KeyCode::Char('q') => app.should_quit = true,
            KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if let Some(pane) = app.active_pane_mut() {
                    pane.write_pty(&[0x01]);
                }
            }
            _ => app.set_status("unknown command"),
        }
        return;
    }

    match key.code {
        KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.prefix = true;
            app.set_status("prefix");
        }
        _ => {
            let bytes = key_to_bytes(key);
            if let Some(pane) = app.active_pane_mut() {
                pane.write_pty(&bytes);
            }
        }
    }
}

fn poll_ptys(app: &mut App) {
    let mut fds: Vec<PollFd> = app
        .panes
        .iter()
        .map(|p| PollFd::new(p.master.as_fd(), PollFlags::POLLIN))
        .collect();

    if let Ok(n) = poll(&mut fds, 0u8) {
        if n > 0 {
            let ready: Vec<usize> = fds
                .iter()
                .enumerate()
                .filter(|(_, fd)| {
                    fd.revents()
                        .unwrap_or(PollFlags::empty())
                        .contains(PollFlags::POLLIN)
                })
                .map(|(i, _)| i)
                .collect();
            for i in ready {
                app.panes[i].read_pty();
            }
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    terminal::enable_raw_mode()?;
    let mut stdout = stdout();
    queue!(stdout, Clear(ClearType::All), DisableLineWrap)?;

    let (w, h) = terminal::size()?;
    let mut app = App::new(w, h)?;

    let (tx, rx) = std::sync::mpsc::channel();

    let tx_input = tx.clone();
    std::thread::spawn(move || {
        loop {
            if let Ok(event) = crossterm::event::read() {
                if tx_input.send(event).is_err() {
                    break;
                }
            }
        }
    });

    while !app.should_quit {
        poll_ptys(&mut app);

        while let Ok(event) = rx.try_recv() {
            match event {
                Event::Key(key) => handle_key(&mut app, key),
                Event::Resize(new_w, new_h) => app.resize_terminal(new_w, new_h),
                _ => {}
            }
        }

        render(&app)?;
        std::thread::sleep(Duration::from_millis(16));
    }

    queue!(stdout, Clear(ClearType::All), MoveTo(0, 0), EnableLineWrap)?;
    terminal::disable_raw_mode()?;
    Ok(())
}
