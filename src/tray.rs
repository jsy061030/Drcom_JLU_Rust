//! Dr.COM 托盘常驻程序。
//!
//! 在托盘里放一个图标，同时内嵌 WebUI 服务：
//! - 左键单击：打开 WebUI 页面
//! - 右键菜单：打开 WebUI / 退出

use drcom::webui::WebUi;
use tao::event::{Event, StartCause};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

enum UserEvent {
    Tray(TrayIconEvent),
    Menu(MenuEvent),
}

fn main() {
    let webui = match WebUi::start("127.0.0.1:0") {
        Ok(webui) => webui,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    let url = webui.url().to_string();
    println!("Dr.COM WebUI: {url}");
    println!("托盘图标已启动：左键打开网页，右键菜单退出。");

    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();

    let proxy = event_loop.create_proxy();
    TrayIconEvent::set_event_handler(Some(move |event| {
        let _ = proxy.send_event(UserEvent::Tray(event));
    }));

    let proxy = event_loop.create_proxy();
    MenuEvent::set_event_handler(Some(move |event| {
        let _ = proxy.send_event(UserEvent::Menu(event));
    }));

    let mut tray_icon: Option<tray_icon::TrayIcon> = None;
    let mut open_id: Option<MenuId> = None;
    let mut quit_id: Option<MenuId> = None;

    event_loop.run(move |event, _target, control_flow| {
        *control_flow = ControlFlow::Wait;

        match event {
            Event::NewEvents(StartCause::Init) => {
                let menu = Menu::new();
                let open_item = MenuItem::new("打开 WebUI", true, None);
                let quit_item = MenuItem::new("退出", true, None);
                if let Err(e) = menu.append(&open_item) {
                    eprintln!("添加菜单项失败: {e}");
                }
                if let Err(e) = menu.append(&quit_item) {
                    eprintln!("添加菜单项失败: {e}");
                }
                open_id = Some(open_item.id().clone());
                quit_id = Some(quit_item.id().clone());

                let icon = make_icon();
                match TrayIconBuilder::new()
                    .with_menu(Box::new(menu))
                    .with_tooltip("Dr.COM 认证")
                    .with_icon(icon)
                    .with_menu_on_left_click(false)
                    .with_menu_on_right_click(true)
                    .build()
                {
                    Ok(icon) => tray_icon = Some(icon),
                    Err(e) => eprintln!("创建托盘图标失败: {e}"),
                }
            }
            Event::UserEvent(UserEvent::Tray(TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            })) => {
                if let Err(e) = webbrowser::open(&url) {
                    eprintln!("打开浏览器失败: {e}");
                }
            }
            Event::UserEvent(UserEvent::Menu(event)) => {
                if Some(&event.id) == open_id.as_ref() {
                    if let Err(e) = webbrowser::open(&url) {
                        eprintln!("打开浏览器失败: {e}");
                    }
                } else if Some(&event.id) == quit_id.as_ref() {
                    tray_icon.take();
                    webui.shutdown();
                    *control_flow = ControlFlow::Exit;
                }
            }
            _ => {}
        }
    });
}

/// 生成一个简单的托盘图标：蓝色圆点 + 白色外圈。
fn make_icon() -> Icon {
    const SIZE: u32 = 32;
    let mut rgba = vec![0u8; (SIZE * SIZE * 4) as usize];

    for y in 0..SIZE {
        for x in 0..SIZE {
            let idx = ((y * SIZE + x) * 4) as usize;
            let dx = x as f32 - (SIZE as f32 - 1.0) / 2.0;
            let dy = y as f32 - (SIZE as f32 - 1.0) / 2.0;
            let distance = (dx * dx + dy * dy).sqrt();

            let (r, g, b, a) = if distance < 11.0 {
                (0x2b, 0x6c, 0xdf, 0xff)
            } else if distance < 14.0 {
                (0xff, 0xff, 0xff, 0xff)
            } else {
                (0, 0, 0, 0)
            };

            rgba[idx] = r;
            rgba[idx + 1] = g;
            rgba[idx + 2] = b;
            rgba[idx + 3] = a;
        }
    }

    Icon::from_rgba(rgba, SIZE, SIZE).expect("valid icon")
}
