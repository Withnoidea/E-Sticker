import argparse
import ctypes
import hashlib
import json
import os
from pathlib import Path
import time
from PIL import ImageGrab

from editor_qa import browser, evaluate, wait_for
from native_qa import Application, dismiss_dialog, foreground, rectangle, send, visible, wait_dialog, window_text


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--exe", default="target/debug/e-sticker.exe")
    parser.add_argument("--output", default="artifacts/interaction-qa")
    arguments = parser.parse_args()
    run = Path(arguments.output).resolve() / (time.strftime("run-%Y%m%d-%H%M%S") + f"-{os.getpid()}")
    data = run / "data"
    data.mkdir(parents=True)
    notes = [
        {"id": 1, "title": "较早创建，最近编辑", "created": 1795219200, "edited": 1795600000},
        {"id": 2, "title": "最新创建，较早编辑", "created": 1795500000, "edited": 1795500000},
        {"id": 3, "title": "相同编辑时间，ID较新", "created": 1795219200, "edited": 1795600000},
        {"id": 4, "title": "置顶旧文章", "created": 1795000000, "edited": 1795000000, "pinned": True},
    ]
    for index, item in enumerate(notes):
        item.update(body="列表续写回归", at={"x": 40 + index * 180, "y": 40}, paper="Mint", tags=["回归"])
    fixture = {"version": 3, "notes": notes}
    (data / "wall.json").write_text(json.dumps(fixture, ensure_ascii=False), encoding="utf-8")
    os.environ["WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS"] = "--remote-debugging-port=9225"
    application = Application(arguments.exe, data)
    results = []

    def passed(name):
        results.append(name)
        print("PASS:", name, flush=True)
        (run / "results.json").write_text(json.dumps({"executable": str(Path(arguments.exe).resolve()), "sha256": hashlib.sha256(Path(arguments.exe).read_bytes()).hexdigest(), "passed": results}, ensure_ascii=False, indent=2), encoding="utf-8")

    def source():
        return evaluate("document.querySelector('textarea.vditor-sv').value")

    def short_note():
        application.click(108)
        wait_for(lambda: visible(application.child(120)), "Article did not return to short note")

    def capture_overlay(path, handle=None):
        user32 = ctypes.WinDLL("user32")
        user32.SetThreadDpiAwarenessContext.argtypes = [ctypes.c_void_p]
        user32.SetThreadDpiAwarenessContext.restype = ctypes.c_void_p
        previous = user32.SetThreadDpiAwarenessContext(-4)
        try:
            ImageGrab.grab(bbox=rectangle(handle or application.hwnd), all_screens=True).save(path)
        finally:
            user32.SetThreadDpiAwarenessContext(previous)

    try:
        application.click(102)
        before = json.loads((data / "wall.json").read_text(encoding="utf-8"))
        for selection, expected in [(0, [4, 3, 1, 2]), (1, [4, 2, 3, 1])]:
            application.select_combo(104, selection)
            for row, note_id in enumerate(expected):
                application.point(130, 190 + row * 88)
                assert window_text(application.child(105)) == notes[note_id - 1]["title"]
                application.click(108)
            application.screenshot(run / f"01-sort-{selection}.png")
        assert json.loads((data / "wall.json").read_text(encoding="utf-8")) == before
        passed("created/edited sorting uses its own clock, keeps pins first, stabilizes ties and never rearranges the wall")
        foreground(application.hwnd)
        application.click(104)
        application.wait(0.3)
        capture_overlay(run / "02-dropdown.png")
        application.key(0x1B)
        assert visible(application.child(104))
        application.key(0x26)
        application.key(0x0D)
        application.wait()
        assert visible(application.child(104))
        passed("styled native dropdown opens and supports Escape, arrow and Enter without leaving the list")
        application.resize(780, 700)
        application.check_layout()
        application.screenshot(run / "03-minimum-list.png")
        application.point(130, 190)
        application.click(120)
        wait_for(lambda: evaluate("!!document.querySelector('.vditor-ir .vditor-reset')"), "Editor did not load")
        browser("click", "[data-type=edit-mode]")
        browser("click", "[data-mode=sv]")
        for kind, prefix in [("list", "* "), ("ordered-list", "1. "), ("check", "* [ ] ")]:
            browser("fill", "textarea.vditor-sv", "第一项")
            browser("click", "[data-type=" + kind + "]")
            assert source() == prefix + "第一项", source()
            assert evaluate("(() => { const field=document.querySelector('textarea.vditor-sv'); return field.selectionStart===field.value.length && field.selectionEnd===field.value.length; })()")
            browser("press", "Enter")
            continuation = "2. " if kind == "ordered-list" else prefix
            assert source() == prefix + "第一项\n" + continuation, source()
            browser("keyboard", "type", "第二项")
            assert source().endswith(continuation + "第二项"), source()
            browser("press", "Enter")
            browser("press", "Enter")
            assert source().endswith("第二项\n"), source()
        passed("real source-mode toolbar lists continue on Enter and exit only on an empty item, including ordered and task lists")
        for hotkey, prefix in [("Control+l", "* "), ("Control+o", "1. "), ("Control+j", "* [ ] ")]:
            browser("fill", "textarea.vditor-sv", "快捷键列表")
            browser("press", hotkey)
            assert source() == prefix + "快捷键列表", source()
            browser("press", "Enter")
            continuation = "2. " if hotkey == "Control+o" else prefix
            assert source() == prefix + "快捷键列表\n" + continuation, source()
        passed("keyboard-created lists place the caret correctly and continue on Enter")
        for initial, expected in [
            ("9. 第一项", "9. 第一项\n10. "),
            ("- 父项\n  - 子项", "- 父项\n  - 子项\n  - "),
            ("- [x] 已完成", "- [x] 已完成\n- [ ] "),
            ("> - 引用中的列表", "> - 引用中的列表\n> - "),
            ("```md\n- 代码不是列表", "```md\n- 代码不是列表\n"),
            ("普通段落", "普通段落\n"),
        ]:
            browser("fill", "textarea.vditor-sv", initial)
            browser("press", "End")
            browser("press", "Enter")
            assert source() == expected, (initial, source())
        passed("nested lists preserve indentation, numbers increment, completed tasks reset, and code/plain text keep normal Enter")
        browser("fill", "textarea.vditor-sv", "- 第一项")
        browser("press", "End")
        browser("press", "Shift+Enter")
        assert source() == "- 第一项\n", source()
        browser("fill", "textarea.vditor-sv", "- 第一项")
        browser("press", "End")
        ignored = evaluate("(() => { const field = document.querySelector('textarea.vditor-sv'); const event = new KeyboardEvent('keydown', {key:'Enter', isComposing:true, bubbles:true, cancelable:true}); return field.dispatchEvent(event) && !event.defaultPrevented; })()")
        assert ignored and source() == "- 第一项"
        browser("press", "Enter")
        assert source() == "- 第一项\n- ", source()
        browser("press", "Control+z")
        wait_for(lambda: source() == "- 第一项", "Undo did not revert automatic continuation")
        browser("press", "Control+y")
        wait_for(lambda: source() == "- 第一项\n- ", "Redo did not restore automatic continuation")
        passed("Shift+Enter and IME composition are not hijacked; automatic continuation participates in undo/redo")
        browser("keyboard", "type", "第二项")
        browser("press", "Control+s")
        wait_for(lambda: "第二项" in json.loads((data / "wall.json").read_text(encoding="utf-8"))["notes"][3]["body"], "Continued list was not saved")
        browser("screenshot", str(run / "04-source-list.png"))
        passed("continued Markdown is persisted through the real Ctrl+S save path")
        for mode, selector in [("ir", ".vditor-ir .vditor-reset"), ("wysiwyg", ".vditor-wysiwyg .vditor-reset")]:
            browser("click", "[data-type=edit-mode]")
            browser("click", "[data-mode=" + mode + "]")
            browser("click", selector + " li:last-child")
            browser("press", "End")
            browser("press", "Enter")
            browser("keyboard", "type", "新条目")
            assert evaluate("document.querySelector('" + selector + " li:last-child').textContent.includes('新条目')")
        passed("instant-render and WYSIWYG list continuation remain intact")
        time.sleep(1)
        menu_visible = "(() => { const panel=document.querySelector('button[data-type=edit-mode]').parentElement.querySelector('.vditor-hint'); return panel.getClientRects().length > 0 && getComputedStyle(panel).display !== 'none'; })()"
        if not evaluate(menu_visible):
            browser("click", "[data-type=edit-mode]")
        wait_for(lambda: evaluate(menu_visible), "Editor dropdown did not open")
        assert evaluate("getComputedStyle(document.querySelector('button[data-type=edit-mode]'), '::after').display === 'none'"), "Tooltip must not cover an open menu"
        browser("screenshot", str(run / "05-editor-dropdown.png"))
        browser("press", "Escape")
        application.wait()
        assert window_text(application.child(108)) == "返回短句", "Escape in a dropdown must not leave the editor"
        short_note()
        application.click(108)
        application.click(114)
        wait_for(lambda: evaluate("!document.querySelector('#settings').hidden"), "Settings did not open")
        evaluate("document.querySelector('select[name=mode]').scrollIntoView({block:'center'})")
        browser("click", "select[name=mode]")
        browser("screenshot", str(run / "06-settings-dropdown.png"))
        browser("press", "Escape")
        application.wait()
        assert visible(application.child(118)), "Escape on a settings select must not leave settings"
        browser("select", "select[name=mode]", "sv")
        browser("select", "select[name=theme]", "dark")
        application.click(118)
        wait_for(lambda: visible(application.child(114)), "Settings did not save")
        assert json.loads((data / "settings.json").read_text(encoding="utf-8"))["editor"]["mode"] == "sv"
        passed("settings dropdown styling preserves native selection and saves at minimum window size")
        application.point(130, 190)
        application.click(120)
        wait_for(lambda: evaluate("!!document.querySelector('.vditor--dark')"), "Dark theme was not applied")
        browser("click", "[data-type=edit-mode]")
        wait_for(lambda: evaluate(menu_visible), "Dark theme dropdown did not open")
        assert evaluate("getComputedStyle(document.querySelector('.vditor--dark .vditor-hint')).backgroundColor === 'rgb(39, 46, 42)'")
        browser("screenshot", str(run / "08-dark-editor.png"))
        browser("press", "Escape")
        short_note()
        passed("dark theme keeps readable dropdown colors and closes menus without leaving the editor")
        application.click(143)
        application.select_combo(160, 2)
        application.point(130, 190)
        application.click(110)
        dialog = wait_dialog(application.process, "不可撤销")
        capture_overlay(run / "07-delete-confirm.png", dialog)
        assert window_text(application.focus()) == "取消", "Destructive confirmation must default to Cancel"
        application.key(0x0D)
        wait_for(lambda: not visible(dialog), "Enter did not activate default Cancel")
        assert len(json.loads((data / "wall.json").read_text(encoding="utf-8"))["notes"]) == 4
        application.click(110)
        dialog = wait_dialog(application.process, "不可撤销")
        application.key(0x1B)
        wait_for(lambda: not visible(dialog), "Escape did not cancel deletion")
        assert len(json.loads((data / "wall.json").read_text(encoding="utf-8"))["notes"]) == 4
        application.click(110)
        dismiss_dialog(application.process, "不可撤销", 7)
        assert len(json.loads((data / "wall.json").read_text(encoding="utf-8"))["notes"]) == 4
        application.click(110)
        dismiss_dialog(application.process, "不可撤销", 6)
        assert len(json.loads((data / "wall.json").read_text(encoding="utf-8"))["notes"]) == 3
        passed("paper-style confirmation defaults to Cancel, cancels safely and deletes only after explicit confirmation")
    except Exception:
        application.screenshot(run / "failure.png")
        print("CONTROLS:", [(identifier, bool(visible(application.child(identifier))), window_text(application.child(identifier))) for identifier in [105, 108, 120, 114]], flush=True)
        try:
            print("EDITOR:", evaluate("({message:document.querySelector('#message').textContent, panels:[...document.querySelectorAll('.vditor-hint')].map(panel=>({display:getComputedStyle(panel).display,visible:panel.getClientRects().length}))})"), flush=True)
            print("ERRORS:", browser("errors"), flush=True)
        except Exception as error:
            print(error, flush=True)
        raise
    finally:
        application.close()
    print(run, flush=True)


if __name__ == "__main__":
    main()
