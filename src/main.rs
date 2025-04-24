use fltk::dialog;
use fltk::{
    app,
    button::Button,
    enums,
    prelude::*,
    text::{TextBuffer, TextDisplay},
    window::Window,
    *,
};
use std::fs::File;
use std::io::{Read, Write};

use std::env;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::Command;
use std::process::{Child, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;

const KEY: &str = "mysecretkey123456"; // 硬编码的加密密钥
const PWD_FILE: &str = ".myfltk_pwd"; // 密码存储文件

struct AppState {
    child_processes: [Arc<Mutex<Option<Child>>>; 2],
    current_dir: PathBuf,
}

fn create_tab(
    tabs: &mut group::Tabs,
    label: &str,
    cmd: &str,
    args: &[&str],
    state: &AppState,
    index: usize,
) -> group::Flex {
    let mut col = group::Flex::default().with_label(label).column();
    col.set_trigger(enums::CallbackTrigger::Never);
    col.set_callback(tab_close_cb);

    let pack = group::Pack::default();
    let mut log_display = TextDisplay::default().with_size(600, 400);
    log_display.set_frame(enums::FrameType::DownBox);

    let buffer = TextBuffer::default();
    log_display.set_buffer(buffer.clone());
    // log_display.set_insert_position(log_display.buffer().unwrap().length());
    // log_display.scroll(
    //     log_display.count_lines(0, log_display.buffer().unwrap().length(), true),
    //     0,
    // );

    start(
        &state.child_processes[index],
        &mut buffer.clone(),
        &log_display,
        cmd,
        args,
    );

    pack.end();
    col.end();
    col
}

fn main() {
    loop {
        // 检查sudo是否有效或密码是否已更改
        let need_input = match get_stored_password() {
            Some(_) => !check_password(),
            None => true,
        };

        if need_input {
            let pwd = match dialog::input_default("请输入sudo密码(取消将退出程序):", "")
            {
                Some(pwd) => pwd,
                None => {
                    println!("用户取消输入，程序退出");
                    return; // 直接退出程序
                }
            };
            save_password(&pwd).unwrap();
        } else {
            break; // 密码正确，继续执行主程序
        }
    }

    let app = app::App::default().with_scheme(app::Scheme::Gtk);
    let current_dir = env::current_exe().unwrap().parent().unwrap().into();

    let state = AppState {
        child_processes: [Arc::new(Mutex::new(None)), Arc::new(Mutex::new(None))],
        current_dir,
    };

    let mut win = Window::default()
        .with_size(600, 400)
        .with_pos(400, 400)
        .with_label("Main Window");

    let row = group::Flex::default_fill().row();
    let mut tabs = group::Tabs::default();
    tabs.set_tab_align(enums::Align::Right);
    tabs.handle_overflow(group::TabsOverflow::Compress);

    // 创建标签页
    create_tab(
        &mut tabs,
        "  Main  ",
        "sudo",
        &["-S", "df", "-h"],
        &state,
        0,
    );
    create_tab(
        &mut tabs,
        "  Log  ",
        "sudo",
        &["-S", "tcpdump", "-i", "en0"],
        &state,
        1,
    );

    // 窗口关闭回调
    win.set_callback({
        let child_processes = state.child_processes.clone();
        move |win| {
            if app::event() == enums::Event::Close {
                child_processes.iter().for_each(|cp| {
                    if let Some(mut child) = cp.lock().unwrap().take() {
                        child.kill().unwrap_or_else(|e| println!("停止失败: {}", e));
                    }
                });
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
    display: &TextDisplay,
    cmd: &str,
    args: &[&str],
) {
    // 获取存储的sudo密码
    let stored_pwd = match get_stored_password() {
        Some(pwd) => pwd,
        None => {
            buffer.set_text("未找到存储的密码，请重新输入");
            return;
        }
    };

    match Command::new(cmd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .args(args)
        .spawn()
    {
        Ok(mut child) => {
            if cmd == "sudo" {
                if let Some(stdin) = child.stdin.as_mut() {
                    if let Err(e) = stdin.write_all(format!("{}\n", stored_pwd).as_bytes()) {
                        buffer.set_text(&format!("密码输入失败: {}", e));
                        return;
                    }
                }
            }

            *child_process.lock().unwrap() = Some(child);
            let child_clone = child_process.clone();
            let mut child = child_clone.lock().unwrap();
            if let Some(child) = child.as_mut() {
                let stdout = child.stdout.take().unwrap();
                let stderr = child.stderr.take().unwrap();

                // 提取公共处理逻辑
                let handle_stream = |stream: Box<dyn Read + Send>,
                                     buffer: TextBuffer,
                                     display: TextDisplay| {
                    thread::spawn(move || {
                        let reader = BufReader::new(stream);
                        for line in reader.lines() {
                            let line = line.unwrap_or_else(|_| "".into());
                            let mut buffer = buffer.clone();
                            let mut display = display.clone();
                            app::awake_callback(move || {
                                if buffer.length() > 1000000 {
                                    buffer.remove(0, 1000000);
                                }
                                buffer.append(&format!("{}\n", line));
                                display.set_insert_position(buffer.length());
                                display.scroll(display.count_lines(0, buffer.length(), true), 0);
                            });
                        }
                    });
                };

                // 处理标准输出
                handle_stream(Box::new(stdout), buffer.clone(), display.clone());
                // 处理错误输出
                handle_stream(Box::new(stderr), buffer.clone(), display.clone());
                
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

// XOR加密/解密函数
fn xor_crypt(input: &str, key: &str) -> String {
    let key_bytes = key.as_bytes();
    input
        .bytes()
        .enumerate()
        .map(|(i, b)| b ^ key_bytes[i % key_bytes.len()])
        .map(|b| b as char)
        .collect()
}

fn check_password() -> bool {
    let stored_pwd = match get_stored_password() {
        Some(pwd) => pwd,
        None => return false,
    };

    // 调整参数顺序，强制进行密码验证
    Command::new("sudo")
        .arg("-k") // 首先清除凭据缓存
        .arg("-S") // 从stdin读取密码
        .arg("true") // 要执行的命令
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .and_then(|mut child| {
            child
                .stdin
                .as_mut()
                .unwrap()
                .write_all(format!("{}\n", stored_pwd).as_bytes())?;
            child.wait()
        })
        .map(|status| status.success())
        .unwrap_or(false)
}

// 获取存储的密码
fn get_stored_password() -> Option<String> {
    let mut path = env::home_dir()?;
    path.push(PWD_FILE);

    File::open(path).ok().and_then(|mut f| {
        let mut s = String::new();
        f.read_to_string(&mut s).ok()?;
        Some(xor_crypt(&s, KEY))
    })
}

// 保存密码
fn save_password(pwd: &str) -> std::io::Result<()> {
    let mut path = env::home_dir().ok_or(std::io::ErrorKind::NotFound)?;
    path.push(PWD_FILE);

    let mut f = File::create(path)?;
    f.write_all(xor_crypt(pwd, KEY).as_bytes())
}
