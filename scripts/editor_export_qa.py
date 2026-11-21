import argparse
import ctypes
import json
import os
from pathlib import Path
import time
from PIL import Image

from editor_qa import browser, evaluate, wait_for
from native_qa import Application, CALLBACK, class_name, enum_children, get_child, post, send, visible, window_text, windows_for


def save_dialog(application):
    dialogs = [handle for handle in windows_for(application.process.pid) if visible(handle) and class_name(handle) == "#32770" and window_text(handle) == "导出 PDF"]
    return dialogs[0] if dialogs else None


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--exe", default="target/debug/e-sticker.exe")
    arguments = parser.parse_args()
    output = Path("artifacts") / f"export-qa-{time.strftime('%Y%m%d-%H%M%S')}"
    output.mkdir(parents=True)
    article = "# PDF export 测试\n\n正文包含中文和 **bold text**。\n\n- [x] Completed task\n- [ ] Pending task\n\n| Item | Status |\n| --- | --- |\n| Export | Ready |\n\n公式：$x^2 + y^2 = 1$\n\n```python\nprint('PDF export')\n```\n"
    (output / "assets").mkdir()
    Image.new("RGB", (160, 80), (80, 140, 100)).save(output / "assets/export-local.png")
    article += "\n![Local image](assets/export-local.png)\n\n" + "\n\n".join(f"Paragraph {index}: multipage PDF export 正文分页验证。" for index in range(1, 61)) + "\n\nFinal paragraph marker.\n"
    fixture = {"version": 2, "notes": [{"id": 1, "title": "PDF export", "paper": "Mint", "at": {"x": 100, "y": 100}, "tags": [], "body": article, "created": 1795219200, "edited": 1795219200}]}
    (output / "wall.json").write_text(json.dumps(fixture, ensure_ascii=False), encoding="utf-8")
    os.environ["WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS"] = "--remote-debugging-port=9225"
    application = Application(arguments.exe, output)
    results = []
    try:
        application.resize(1200, 1000)
        application.point(160, 300)
        application.click(120)
        wait_for(lambda: evaluate("!!document.querySelector('.vditor-ir .vditor-reset')"), "Editor did not load")
        for width in (1200, 780):
            application.resize(width, 1000)
            for name in ("headings", "check", "table", "outline", "export"):
                selector = f".vditor-toolbar [data-type='{name}']"
                browser("hover", selector)
                metrics = evaluate(f"""(() => {{
                    const button = document.querySelector({json.dumps(selector)});
                    const style = getComputedStyle(button, '::after');
                    const rect = button.getBoundingClientRect();
                    return {{className:button.className, top:rect.top + parseFloat(style.top), height:parseFloat(style.height), viewport:innerHeight, content:style.content}};
                }})()""")
                assert "vditor-tooltipped__s" in metrics["className"], metrics
                if metrics["content"] != "none":
                    assert 0 <= metrics["top"] < metrics["viewport"] - metrics["height"], metrics
            browser("screenshot", str(output / f"tooltip-{width}.png"))
        results.append("Toolbar tips point down at normal and minimum widths")
        application.resize(1200, 1000)
        browser("click", "[data-type=export]")
        browser("click", ".vditor-hint button[data-type=pdf]")
        wait_for(lambda: save_dialog(application), "PDF Save As dialog did not appear")
        post(get_child(save_dialog(application), 2), 0x00F5, 0, 0)
        wait_for(lambda: evaluate("document.querySelector('.vditor-tip').textContent.includes('已取消')"), "Cancel did not clear PDF export state")
        assert evaluate("document.querySelector('#print-content').childElementCount") == 0
        results.append("Cancel clears progress and print content")
        for mode in ("ir", "sv", "wysiwyg"):
            if mode != "ir":
                browser("click", "[data-type=edit-mode]")
                browser("click", f"[data-mode={mode}]")
            browser("click", "[data-type=export]")
            browser("click", ".vditor-hint button[data-type=pdf]")
            wait_for(lambda: save_dialog(application), f"{mode}: PDF Save As dialog did not appear")
            assert evaluate("!!document.querySelector('#print-content table') && !!document.querySelector('#print-content input[type=checkbox]') && !!document.querySelector('#print-content .katex')"), "Print content lost table, tasks or formula"
            assert evaluate("document.querySelector('#print-content img').naturalWidth") == 160, "Local image failed to load for printing"
            dialog = save_dialog(application)
            edits = []

            @CALLBACK
            def collect(handle, parameter):
                if visible(handle) and class_name(handle) == "Edit":
                    edits.append(handle)
                return True

            enum_children(dialog, collect, 0)
            assert len(edits) == 1, f"Unexpected filename inputs: {edits}"
            destination = (output / f"{mode}.pdf").resolve()
            filename = ctypes.create_unicode_buffer(str(destination))
            send(edits[0], 0x00B1, 0, -1)
            send(edits[0], 0x00C2, 1, ctypes.addressof(filename))
            assert window_text(edits[0]) == str(destination)
            post(get_child(dialog, 1), 0x00F5, 0, 0)
            wait_for(lambda: evaluate("document.querySelector('.vditor-tip').textContent.includes('PDF 已导出')"), f"{mode}: PDF did not complete", timeout=25)
            assert evaluate("document.querySelector('.vditor-tip').textContent").endswith(str(destination))
            content = destination.read_bytes()
            assert content.startswith(b"%PDF-") and len(content) > 5000, f"Invalid PDF: {len(content)} bytes"
            assert evaluate("document.querySelector('#print-content').childElementCount") == 0
            results.append(f"{mode}: actual PDF saved ({len(content)} bytes)")
        assert json.loads((output / "wall.json").read_text(encoding="utf-8"))["notes"][0]["body"] == article
        results.append("Export does not modify original Markdown")
        (output / "results.json").write_text(json.dumps(results, ensure_ascii=False, indent=2), encoding="utf-8")
        print(json.dumps({"output": str(output), "results": results}, ensure_ascii=False, indent=2), flush=True)
    finally:
        dialog = save_dialog(application)
        if dialog:
            post(get_child(dialog, 2), 0x00F5, 0, 0)
        application.close()


if __name__ == "__main__":
    main()
