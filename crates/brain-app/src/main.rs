mod conversation;
mod model;
mod system;
mod theme;
mod view;

use gpui::{
    actions, point, prelude::*, px, size, App, Application, Bounds, KeyBinding, Menu, MenuItem,
    SharedString, TitlebarOptions, WindowBounds, WindowOptions,
};

actions!(brain, [Quit]);

fn main() {
    Application::new().run(|cx: &mut App| {
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.bind_keys([KeyBinding::new("cmd-q", Quit, None)]);
        cx.set_menus(vec![Menu {
            name: "Brain".into(),
            items: vec![MenuItem::action("Quit Brain", Quit)],
        }]);

        let options = WindowOptions {
            titlebar: Some(TitlebarOptions {
                title: Some(SharedString::from("Brain")),
                appears_transparent: true,
                traffic_light_position: Some(point(px(14.), px(14.))),
            }),
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(1180.), px(760.)),
                cx,
            ))),
            ..Default::default()
        };

        cx.open_window(options, |window, cx| cx.new(|cx| view::BrainView::new(window, cx)))
            .expect("failed to open window");
        cx.on_window_closed(|cx| cx.quit()).detach();
        cx.activate(true);
    });
}
