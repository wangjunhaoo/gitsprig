use tauri::{
    menu::{MenuBuilder, MenuItemBuilder, SubmenuBuilder},
    Emitter,
};

pub fn install(app: &tauri::AppHandle) -> tauri::Result<()> {
    let item = |id: &str, title: &str, shortcut: &str| {
        MenuItemBuilder::with_id(id, title)
            .accelerator(shortcut)
            .build(app)
    };
    let about = SubmenuBuilder::new(app, "GitSprig")
        .about_with_text("关于轻枝 GitSprig", None)
        .separator()
        .hide_with_text("隐藏 GitSprig")
        .hide_others_with_text("隐藏其他应用")
        .separator()
        .item(&item("quit", "退出 GitSprig", "CmdOrCtrl+Q")?)
        .build()?;
    let file = SubmenuBuilder::new(app, "文件")
        .item(&item("open", "打开仓库…", "CmdOrCtrl+O")?)
        .text("clone", "克隆仓库…")
        .text("init", "初始化仓库…")
        .separator()
        .item(&item("close", "关闭窗口", "CmdOrCtrl+W")?)
        .build()?;
    let edit = SubmenuBuilder::new(app, "编辑")
        .undo_with_text("撤销")
        .redo_with_text("重做")
        .separator()
        .cut_with_text("剪切")
        .copy_with_text("复制")
        .paste_with_text("粘贴")
        .select_all_with_text("全选")
        .build()?;
    let repository = SubmenuBuilder::new(app, "仓库")
        .item(&item("commit-focus", "准备提交", "CmdOrCtrl+K")?)
        .item(&item("commit-now", "提交所选改动", "CmdOrCtrl+Enter")?)
        .item(&item("push", "推送…", "CmdOrCtrl+Shift+K")?)
        .text("fetch", "获取远程更新")
        .text("pull", "快进拉取")
        .separator()
        .text("branch", "创建分支…")
        .text("stash", "暂存当前工作…")
        .text("rebase", "交互式变基…")
        .build()?;
    let view = SubmenuBuilder::new(app, "视图")
        .item(&item("changes", "本地变更", "CmdOrCtrl+0")?)
        .item(&item("history", "提交历史", "CmdOrCtrl+9")?)
        .item(&item("refresh", "刷新仓库", "CmdOrCtrl+R")?)
        .item(&item("palette", "查找操作…", "CmdOrCtrl+Shift+A")?)
        .separator()
        .fullscreen_with_text("进入全屏")
        .build()?;
    let window = SubmenuBuilder::new(app, "窗口")
        .minimize_with_text("最小化")
        .build()?;
    app.set_menu(
        MenuBuilder::new(app)
            .items(&[&about, &file, &edit, &repository, &view, &window])
            .build()?,
    )?;
    app.on_menu_event(|app, event| {
        let _ = app.emit("native-menu", event.id().as_ref());
    });
    Ok(())
}
