use fltk::{
    app,
    button::Button,
    enums,
    prelude::*,
    text::{TextBuffer, TextDisplay},
    window::Window,
    *,
};
use std::env;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::Command;
use std::process::{Child, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;

fn main() {
    let app = app::App::default().with_scheme(app::Scheme::Gtk);
    let current_dir = env::current_exe().unwrap();
    let current_dir = PathBuf::from(current_dir.parent().unwrap());

    let mut win = Window::default()
        .with_size(600, 400)
        .with_pos(400, 400)
        .with_label("Main Window");

    let row = group::Flex::default_fill().row();
    let mut tabs = group::Tabs::default();
    let child_process = [Arc::new(Mutex::new(None)), Arc::new(Mutex::new(None))];
    tabs.set_tab_align(enums::Align::Right);
    tabs.handle_overflow(group::TabsOverflow::Compress);

    // first tab
    {
        let mut col1 = group::Flex::default().with_label("  Main  ").column();
        col1.set_trigger(enums::CallbackTrigger::Never);
        col1.set_callback(tab_close_cb);
        // widgets
        let pack = group::Pack::default();

        let mut log_display = TextDisplay::default().with_size(600, 400);
        log_display.set_frame(enums::FrameType::DownBox);

        let buffer = TextBuffer::default();
        log_display.set_buffer(buffer.clone());

        let script_path = current_dir.parent().unwrap().join("enter.sh");

        start(
            &child_process[0].clone(),
            &mut buffer.clone(),
            "df",
            &["-h"],
        );

        pack.end();
        col1.end();
    }
    // end first tab

    // second tab
    {
        let mut col2 = group::Flex::default()
            .with_label("  Running Log  ")
            .column();
        col2.set_trigger(enums::CallbackTrigger::Never);
        col2.set_callback(tab_close_cb);
        // widgets
        let pack = group::Pack::default();

        let mut log_display = TextDisplay::default().with_size(600, 400);
        log_display.set_frame(enums::FrameType::DownBox);

        let mut buffer = TextBuffer::default();
        log_display.set_buffer(buffer.clone());

        pack.end();
        col2.end();

        start(&child_process[1], &mut buffer, "top", &[]);
    }
    // end second tab

    win.set_callback({
        let child_process = child_process.clone();
        move |win| {
            if app::event() == enums::Event::Close {
                for child_process in child_process.iter() {
                    if let Some(mut child) = child_process.lock().unwrap().take() {
                        if let Err(e) = child.kill() {
                            println!("停止失败: {}", e);
                        }
                    }
                }
                win.hide();
                app::quit();
            }
        }
    });

    tabs.end();
    tabs.auto_layout();
    row.end();
    win.end();
    win.show();

    app.run().unwrap();
}

fn start(
    child_process: &Arc<Mutex<Option<Child>>>,
    buffer: &mut TextBuffer,
    cmd: &str,
    args: &[&str],
) {
    match Command::new(cmd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .args(args)
        .spawn()
    {
        Ok(child) => {
            *child_process.lock().unwrap() = Some(child);
            let child_clone = child_process.clone();
            let mut child = child_clone.lock().unwrap();
            if let Some(child) = child.as_mut() {
                let stdout = child.stdout.take().unwrap();
                let stderr = child.stderr.take().unwrap();
                // 处理标准输出
                let buffer1 = buffer.clone();
                thread::spawn(move || {
                    let reader = BufReader::new(stdout);
                    for line in reader.lines() {
                        let line = line.unwrap_or_else(|_| "".into());
                        let mut buffer = buffer1.clone();
                        app::awake_callback(move || {
                            buffer.append(&format!("{}\n", line));
                        })
                    }
                });

                // 处理错误输出
                let buffer2 = buffer.clone();
                thread::spawn(move || {
                    let reader = BufReader::new(stderr);
                    for line in reader.lines() {
                        let line = line.unwrap_or_else(|_| "".into());
                        let mut buffer = buffer2.clone();
                        app::awake_callback(move || {
                            buffer.append(&format!("[ERROR] {}\n", line));
                        })
                    }
                });
            }
        }
        Err(e) => {
            buffer.set_text(&format!("启动失败: {}", e));
        }
    }
}

fn tab_close_cb(g: &mut impl GroupExt) {
    if app::callback_reason() == enums::CallbackReason::Closed {
        let mut parent = g.parent().unwrap();
        parent.remove(g);
        app::redraw();
    }
}
