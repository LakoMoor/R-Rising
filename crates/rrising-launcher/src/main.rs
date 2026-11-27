mod backend;
use gpui::{
    div, prelude::*, px, rgb, size, App, Bounds, Context, PathPromptOptions, Window, WindowBounds,
    WindowOptions,
};
use std::path::PathBuf;
struct Launcher {
    install: PathBuf,
    status: String,
    busy: bool,
    verified: bool,
}
impl Launcher {
    fn check(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.verified = false;
        self.status = "Проверяю версию и целостность файлов…".into();
        cx.notify();
        let path = self.install.clone();
        let task = cx
            .background_executor()
            .spawn(async move { backend::verify(&path) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                this.verified = result.is_ok();
                this.status = result
                    .map(|_| "Версия подтверждена. Можно запускать прототип.".to_string())
                    .unwrap_or_else(|e| format!("{e:#}"));
                cx.notify();
            });
        })
        .detach();
    }
    fn choose(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let prompt = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Папка V Rising с GameAssembly.dll".into()),
        });
        cx.spawn(async move |this, cx| {
            let result = prompt.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(Ok(Some(paths))) => {
                        if let Some(path) = paths.into_iter().next() {
                            this.install = path;
                            this.verified = false;
                            this.status = match backend::remember_install(&this.install) {
                                Ok(()) => "Папка выбрана. Проверьте версию перед запуском.".into(),
                                Err(e) => format!("Папка выбрана, но путь не сохранён: {e:#}"),
                            };
                        }
                    }
                    Ok(Ok(None)) => {}
                    other => this.status = format!("Не удалось выбрать папку: {other:?}"),
                };
                cx.notify();
            });
        })
        .detach();
    }
    fn launch(&mut self, cx: &mut Context<Self>) {
        if self.busy || !self.verified {
            return;
        }
        self.busy = true;
        self.status="Прототип запускается или работает. WASD — движение. Журнал: target/launcher-prototype.log".into();
        cx.notify();
        let path = self.install.clone();
        let task = cx
            .background_executor()
            .spawn(async move { backend::launch(&path) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                this.status = result.unwrap_or_else(|e| format!("{e:#}"));
                cx.notify();
            });
        })
        .detach();
    }
}
impl Render for Launcher {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().flex().flex_col().p_8().gap_6().bg(rgb(0x101219)).text_color(rgb(0xe6e3df)).font_family("Segoe UI")
   .child(div().text_sm().text_color(rgb(0xca777c)).child("RUST • НАТИВНЫЙ ПРОТОТИП"))
   .child(div().text_3xl().child("R–RISING"))
   .child(div().text_color(rgb(0xb1adb6)).child("V Rising 1.1.13.0-r99712-b17 • локальная установка"))
   .child(div().p_4().bg(rgb(0x1b1e28)).rounded_md().child(self.install.display().to_string()))
   .child(div().flex().gap_3()
    .child(div().id("choose").p_3().rounded_md().bg(rgb(0x303441)).cursor_pointer().on_click(cx.listener(|this,_,_,cx|this.choose(cx))).child("Выбрать папку"))
    .child(div().id("verify").p_3().rounded_md().bg(rgb(0x303441)).cursor_pointer().on_click(cx.listener(|this,_,_,cx|this.check(cx))).child("Проверить файлы"))
    .child(div().id("launch").p_3().rounded_md().bg(rgb(if self.verified&&!self.busy{0x853342}else{0x34303a})).cursor_pointer().on_click(cx.listener(|this,_,_,cx|this.launch(cx))).child("Запустить")))
   .child(div().p_4().rounded_md().bg(rgb(0x1b1e28)).child(self.status.clone()))
   .child(div().text_sm().text_color(rgb(0xa49ca7)).child("Экспериментальное движение, выборки ходьбы и статические рамки интерфейса. Полноценная игра и совместимость поведения ещё не достигнуты."))
    }
}
fn main() {
    if std::env::args().any(|a| a == "--check") {
        match backend::verify(&backend::default_install()) {
            Ok(()) => println!("Pinned installation verified"),
            Err(e) => {
                eprintln!("{e:#}");
                std::process::exit(1);
            }
        }
        return;
    }
    gpui_platform::application().run(|cx: &mut App| {
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        let bounds = Bounds::centered(None, size(px(860.), px(560.)), cx);
        let result = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |window, cx| {
                window.set_window_title("R-Rising Launcher");
                cx.new(|_| Launcher {
                    install: backend::default_install(),
                    status: "Выберите папку игры и проверьте файлы.".into(),
                    busy: false,
                    verified: false,
                })
            },
        );
        if let Err(e) = result {
            eprintln!("Окно лаунчера: {e:#}");
            cx.quit();
        } else {
            cx.activate(true);
        }
    });
}
