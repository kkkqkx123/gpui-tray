[English](README.md) | 简体中文

# gpui-tray

为 [GPUI](https://github.com/zed-industries/zed) 应用提供的系统托盘 / 菜单栏图标集成。

- 一套 API 覆盖 macOS、Windows 和 Linux；每个操作都接受所选 GPUI flavor 的 `gpui::App`。
- 菜单项既可以携带 GPUI `Action`（走 GPUI 常规的动作分发管线），也可以只挂一个普通回调，
  因此托盘菜单的行为与应用菜单、快捷键路径完全一致。
- 三个平台都使用原生菜单：Linux 上是 `DBusMenu`，macOS 上是 `NSMenu`，Windows 上是 `HMENU`。
- 托盘永不阻塞启动。当平台没有可用的托盘时（例如 GNOME 未安装 AppIndicator 扩展），
  `Tray::is_available()` 返回 `false`，应用照常运行。

## 用法

```toml
[dependencies]
gpui-kit = "0.8"
gpui-tray = { version = "0.1", default-features = false, features = ["gpui-kit"] }
```

```rust
use gpui_kit::*;
use gpui_tray::*;

actions!(demo, [ShowWindow, Quit]);

fn main() {
    gpui_kit::application().run(|cx| {
        gpui_kit::init(cx);

        let tray = Tray::new(cx).expect("failed to create tray");
        tray.set_tooltip(Some("Demo".into()));
        tray.set_left_click_action(LeftClick::Action(std::sync::Arc::new(ShowWindow)));
        tray.set_menu(tray_menu(|m| {
            m.action(ShowWindow, "Show Window");
            m.separator();
            m.action(Quit, "Quit");
        }));
    });
}
```

`Tray::new` 必须在 GPUI 线程上、在 `application().run(|cx| ..)` 内部创建，
与其他进入应用状态的句柄一致。

## GPUI flavor

必须且只能选择一个 flavor，并且要选应用自身所使用的那一个：托盘接收的是该 flavor 的
`gpui::App`，分发的也是该 flavor 的 `Action`。

| Feature     | 绑定的 GPUI                   |
| ----------- | ------------------------------ |
| `gpui-kit`  | GPUI Kit 的快照（`gpui-pre`）  |
| `gpui-fast` | GPUI Fast（`gpui-fast`）       |

默认是 `gpui-kit`。选择另一个 flavor 时需要关闭默认 feature：

```toml
gpui-tray = { version = "0.1", default-features = false, features = ["gpui-fast"] }
```

## 平台说明

| 平台   | 后端                              | 左键行为             | 菜单         |
| ------ | --------------------------------- | -------------------- | ------------ |
| Linux  | `ksni`（StatusNotifierItem）      | 恢复窗口             | `DBusMenu`   |
| macOS  | `objc2` `NSStatusItem`            | 弹出菜单（可配置）   | `NSMenu`     |
| Windows | `tray-icon`（`Shell_NotifyIcon`） | 弹出菜单（可配置）   | `HMENU`      |

Linux 上菜单由桌面 Shell 绘制，因此只有会话提供了 StatusNotifierItem 宿主时托盘图标才会出现：
KDE Plasma 原生支持，GNOME 需要安装 AppIndicator 扩展。

## 示例

`examples/tray_demo` 展示了一个带原生菜单、生成图标、提示文字和左键联动的托盘图标：

```bash
cargo run --example tray_demo
```

## 许可证

Apache-2.0
