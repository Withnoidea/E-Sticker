import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time
from PIL import Image

from native_qa import Application, dismiss_dialog, foreground, post, visible, window_text
from multiwall_qa import select_menu_item


def browser(*arguments):
    executable = shutil.which("agent-browser")
    if not executable:
        raise RuntimeError("Install agent-browser before running the WebView2 integration checks")
    native = Path(executable).parent / "node_modules/agent-browser/bin/agent-browser-win32-x64.exe"
    if native.exists():
        executable = str(native)
    if executable.endswith((".cmd", ".bat")):
        command = ["cmd", "/c", executable]
    else:
        command = [executable]
    with tempfile.TemporaryFile(mode="w+", encoding="utf-8") as stdout, tempfile.TemporaryFile(mode="w+", encoding="utf-8") as stderr:
        result = subprocess.run(command + ["--session", f"esticker-editor-qa-{os.getpid()}", "--cdp", "9225", "--json", *arguments], stdout=stdout, stderr=stderr, timeout=35)
        stdout.seek(0)
        stderr.seek(0)
        output, error = stdout.read(), stderr.read()
    if result.returncode:
        raise AssertionError(error or output)
    response = json.loads(output)
    if not response.get("success"):
        raise AssertionError(response)
    return response.get("data", {})


def evaluate(expression):
    return browser("eval", expression).get("result")


def wait_for(predicate, description, timeout=15):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if predicate():
            return
        time.sleep(0.2)
    raise AssertionError(description)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--exe", default="target/debug/e-sticker.exe")
    parser.add_argument("--output", default="artifacts/editor-qa")
    parser.add_argument("--probe", action="store_true")
    arguments = parser.parse_args()
    run = Path(arguments.output).resolve() / (time.strftime("run-%Y%m%d-%H%M%S") + f"-{os.getpid()}")
    data = run / "data"
    data.mkdir(parents=True)
    original = "# 正文编辑体验\n\n保留 **Markdown**，不动便签位置。\n\n- [ ] 一件值得记下的小事\n\n| 项目 | 状态 |\n| --- | --- |\n| 编辑器 | 待验证 |\n\n```rust\nfn main() { println!(\"hello\"); }\n```\n"
    fixture = {"version": 2, "notes": [{"id": 1, "title": "一张便签，也能认真写长文", "body": original, "paper": "Mint", "at": {"x": 100, "y": 100}, "created": 1795219200, "edited": 1795219200, "tags": ["工作", "随记"]}]}
    (data / "wall.json").write_text(json.dumps(fixture, ensure_ascii=False), encoding="utf-8")
    os.environ["WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS"] = "--remote-debugging-port=9225"
    application = Application(arguments.exe, data)
    (run / "probe.json").write_text(json.dumps({"pid": application.process.pid, "hwnd": application.hwnd, "data": str(data)}), encoding="utf-8")
    application.point(160, 300)
    application.click(120)
    if arguments.probe:
        print(run, flush=True)
        return
    results = []

    def passed(name):
        results.append(name)
        print("PASS:", name, flush=True)
        (run / "results.json").write_text(json.dumps({"executable": str(Path(arguments.exe).resolve()), "sha256": hashlib.sha256(Path(arguments.exe).read_bytes()).hexdigest(), "passed": results}, ensure_ascii=False, indent=2), encoding="utf-8")

    def wall():
        return json.loads((data / "wall.json").read_text(encoding="utf-8"))

    def return_to_wall():
        application.click(108)
        wait_for(lambda: visible(application.child(120)), "Did not return from article to short note")
        application.click(108)
        wait_for(lambda: visible(application.child(114)), "Did not return to wall")

    try:
        wait_for(lambda: evaluate("!!document.querySelector('.vditor-ir .vditor-reset')"), "Vditor did not load")
        passed("embedded Vditor starts with offline assets in the real native WebView2 window")
        browser("screenshot", str(run / "01-editor.png"))
        application.click(108)
        wait_for(lambda: visible(application.child(120)), "Unedited article did not return")
        assert wall()["notes"][0]["body"] == original
        passed("opening and closing an unedited article preserves its exact original Markdown")
        application.click(120)
        wait_for(lambda: evaluate("!!document.querySelector('.vditor-ir .vditor-reset')"), "Editor did not reopen")
        browser("fill", ".vditor-ir .vditor-reset", "中文输入与自动保存🙂")
        wait_for(lambda: "中文输入与自动保存🙂" in wall()["notes"][0]["body"], "Real editor input was not autosaved")
        assert wall()["notes"][0]["at"] == fixture["notes"][0]["at"]
        assert wall()["notes"][0]["tags"] == fixture["notes"][0]["tags"]
        passed("real editor input autosaves Unicode text without changing note position or tags")
        return_to_wall()
        foreground(application.hwnd)
        application.click(154)
        select_menu_item(application, 0)
        wait_for(lambda: visible(application.child(155)), "Wall naming form did not open")
        application.type(155, "编辑中的分类")
        application.click(156)
        application.select_combo(153, 0)
        application.click(101)
        application.type(105, "原生墙面仍然可以亲手落贴")
        application.type(149, "回归, native")
        application.click(107)
        application.point(500, 240)
        assert len(wall()["notes"]) == 2
        assert wall()["notes"][0]["at"] == fixture["notes"][0]["at"]
        wall_before_search = wall()
        application.click(102)
        application.type(103, "中文输入")
        assert wall() == wall_before_search
        application.click(102)
        for step in range(4):
            if window_text(application.child(102)) == "文章列表":
                break
            application.click(102)
        passed("native note creation, tags, manual placement and body search preserve the existing wall")
        application.click(114)
        wait_for(lambda: evaluate("!document.querySelector('#settings').hidden"), "Settings page did not open")
        browser("select", "select[name=mode]", "sv")
        browser("select", "select[name=font_size]", "20")
        browser("select", "select[name=autosave_ms]", "5000")
        browser("screenshot", str(run / "02-settings.png"))
        application.click(118)
        wait_for(lambda: (data / "settings.json").exists(), "Editor preferences were not saved")
        preferences = json.loads((data / "settings.json").read_text(encoding="utf-8"))
        assert preferences["editor"]["mode"] == "sv" and preferences["editor"]["font_size"] == 20
        passed("editor settings persist through native Save settings")
        application.click(114)
        wait_for(lambda: evaluate("!document.querySelector('#settings').hidden"), "Settings did not reopen")
        browser("select", "select[name=font_size]", "12")
        application.click(119)
        wait_for(lambda: visible(application.child(114)), "Settings cancel did not return")
        assert json.loads((data / "settings.json").read_text(encoding="utf-8")) == preferences
        passed("cancel settings does not persist draft changes")
        application.point(160, 300)
        application.click(120)
        wait_for(lambda: evaluate("!!document.querySelector('textarea.vditor-sv')"), "Saved source mode was not applied")
        markdown = "# 成熟编辑器\n\n**粗体**与 *斜体*\n\n- [ ] 待办\n\n| 项目 | 状态 |\n| --- | --- |\n| Vditor | 已接入 |\n\n```rust\nfn main() {}\n```\n\n$$E=mc^2$$\n"
        browser("fill", "textarea.vditor-sv", markdown)
        browser("press", "Control+s")
        wait_for(lambda: "成熟编辑器" in wall()["notes"][0]["body"], "Ctrl+S did not save source mode")
        assert evaluate("Number(document.querySelector('.vditor-counter').textContent)") > 0
        passed("source mode and Ctrl+S save Markdown tables, task lists, code fences and math")
        prior_body = wall()["notes"][0]["body"]
        blocked_temp = data / "wall.tmp"
        blocked_temp.mkdir()
        browser("fill", "textarea.vditor-sv", markdown + "\n保存失败后应保留草稿，再试一次。")
        browser("press", "Control+s")
        dismiss_dialog(application.process, "写入失败")
        assert wall()["notes"][0]["body"] == prior_body
        assert "保存失败后应保留草稿" in evaluate("document.querySelector('textarea.vditor-sv').value")
        blocked_temp.rmdir()
        browser("press", "Control+s")
        wait_for(lambda: "保存失败后应保留草稿" in wall()["notes"][0]["body"], "Retry did not save the retained draft")
        passed("disk write failure preserves the actual editor draft and Ctrl+S retries successfully")
        image_path = run / "image-fixture.png"
        Image.new("RGB", (80, 48), (82, 111, 90)).save(image_path)
        browser("upload", "input[type=file]", str(image_path))
        wait_for(lambda: bool(list((data / "assets").glob("*.png"))), "Real editor upload did not reach local assets")
        browser("press", "Control+s")
        wait_for(lambda: "assets/image-" in wall()["notes"][0]["body"], "Local image reference was not saved")
        assert "notes.esticker.local" not in wall()["notes"][0]["body"]
        with Image.open(next((data / "assets").glob("*.png"))) as image:
            assert image.getpixel((4, 4)) == (82, 111, 90)
        passed("Vditor image upload stores original pixels locally and persists a portable relative reference")
        browser("click", "[data-type=edit-mode]")
        browser("click", "[data-mode=wysiwyg]")
        wait_for(lambda: evaluate("!!document.querySelector('.vditor-wysiwyg .vditor-reset table')"), "WYSIWYG table rendering failed")
        wait_for(lambda: evaluate("!!document.querySelector('.vditor-wysiwyg img')?.naturalWidth"), "Local images did not render")
        browser("screenshot", str(run / "03-wysiwyg.png"))
        passed("the real toolbar switches to WYSIWYG with tables and local images rendered")
        violation = evaluate("new Promise(resolve => { document.addEventListener('securitypolicyviolation', event => resolve(event.violatedDirective), {once:true}); const image = new Image(); image.src='https://example.com/blocked.png'; document.body.append(image); })")
        assert violation == "img-src", violation
        passed("remote images are blocked by the embedded page security policy")
        browser("click", "[data-type=edit-mode]")
        browser("click", "[data-mode=sv]")
        application.resize(780, 700)
        application.check_layout()
        browser("screenshot", str(run / "04-minimum.png"))
        passed("the native controls and hosted editor remain usable at the minimum window size")
        live_marker = "移动前刚输入的正文也不能丢失🙂"
        browser("fill", "textarea.vditor-sv", "# 分类移动\n\n" + live_marker)
        foreground(application.hwnd)
        application.click(159)
        select_menu_item(application, 1)
        wait_for(lambda: wall()["notes"][0]["wall_id"] == wall()["walls"][0]["id"], "Live article did not move into category")
        assert live_marker in wall()["notes"][0]["body"]
        assert wall()["notes"][0]["at"] == fixture["notes"][0]["at"]
        application.select_combo(153, 1)
        application.point(120, 250)
        application.click(120)
        wait_for(lambda: live_marker in (evaluate("document.querySelector('textarea.vditor-sv')?.value") or ""), "Category editor did not show the same saved article")
        application.click(159)
        select_menu_item(application, 0)
        wait_for(lambda: wall()["notes"][0]["wall_id"] == 0, "Live article did not return to default category")
        assert len(wall()["notes"]) == 2
        assert live_marker in wall()["notes"][0]["body"]
        passed("moving a live Markdown article flushes pending input, shares content with category editor and never duplicates it")
        application.select_combo(153, 0)
        application.point(160, 300)
        application.click(120)
        wait_for(lambda: evaluate("!!document.querySelector('textarea.vditor-sv')"), "Editor did not reopen before close test")
        browser("fill", "textarea.vditor-sv", "# 最后一笔\n\n关闭前立即输入，也不能丢稿。")
        post(application.hwnd, 0x0010, 0, 0)
        application.process.wait(timeout=15)
        assert application.process.returncode == 0
        assert "关闭前立即输入，也不能丢稿。" in wall()["notes"][0]["body"]
        passed("closing before the five-second autosave flushes the live editor and exits cleanly")
    finally:
        if application.process.poll() is None:
            try:
                browser("screenshot", str(run / "failure.png"))
                (run / "diagnostics.json").write_text(json.dumps(browser("eval", "document.body.innerText"), ensure_ascii=False), encoding="utf-8")
            except Exception as error:
                print("Diagnostic capture failed:", error, flush=True)
            post(application.hwnd, 0x0010, 0, 0)
            try:
                application.process.wait(timeout=15)
            except subprocess.TimeoutExpired:
                application.process.terminate()
    print(run, flush=True)


if __name__ == "__main__":
    main()
