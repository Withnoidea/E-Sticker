import argparse
import json
import os
from pathlib import Path
import time
from PIL import Image

from editor_qa import browser, evaluate, wait_for
from native_qa import Application, visible


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--exe", default="target/release/e-sticker.exe")
    arguments = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    data = root / "artifacts" / f"readme-demo-{time.strftime('%Y%m%d-%H%M%S')}"
    output = root / "docs" / "screenshots"
    data.mkdir(parents=True)
    output.mkdir(parents=True, exist_ok=True)
    article = "# 把灵感留住\n\n一张便签记下 **当下的念头**，一篇正文把它慢慢展开。\n\n## 今天，给自己一点留白\n\n不急着把所有事情整理好。先写下来，再决定放在哪里。\n\n- [x] 留住一个突然出现的想法\n- [ ] 为喜欢的事情安排一点时间\n- [ ] 晚上回来，写一段小小的复盘\n\n## 从想法到行动\n\n| 阶段 | 记录方式 |\n| --- | --- |\n| 灵感刚出现 | 随手写一句，亲手贴上墙 |\n| 想继续展开 | 打开同一张便签，写 Markdown |\n| 需要回头找 | 用正文搜索和标签筛选 |\n\n> 不必每条记录都成为计划。有些想法，值得只是被记住。\n"
    examples = [
        ("把灵感贴上墙\n再慢慢写成文章", "Yellow", 70, 62, ["灵感", "写作"], article),
        ("散步时想到的事\n回来记得写下来", "Pink", 288, 142, ["生活"], "散步时不着急找答案，先看看路边的树。"),
        ("今天的小目标\n认真做好一件事", "Mint", 515, 73, ["工作"], "- [ ] 把手上的事情做好\n- [ ] 给明天留一点余地"),
        ("读到喜欢的句子\n就留在这里", "Blue", 745, 177, ["阅读"], "记录读到的片段，也写下当时的感受。"),
        ("周末计划\n去逛一家没去过的书店", "Lilac", 940, 62, ["周末"], "不设太多目标，给偶遇留出空间。"),
        ("复盘不是打分\n是看见自己的变化", "Mint", 155, 352, ["复盘"], "今天学到了什么？下次想尝试什么？"),
        ("不必自动归位\n按自己的节奏摆放", "Yellow", 535, 333, ["随记"], "便签可以重叠；位置和顺序由自己决定。"),
    ]
    notes = [{"id": index, "title": title, "paper": paper, "at": {"x": xpos, "y": ypos}, "tags": tags, "body": body, "created": 1795219200, "edited": 1795219200} for index, (title, paper, xpos, ypos, tags, body) in enumerate(examples, start=1)]
    (data / "wall.json").write_text(json.dumps({"version": 2, "notes": notes}, ensure_ascii=False), encoding="utf-8")
    os.environ["WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS"] = "--remote-debugging-port=9225"
    application = Application(arguments.exe, data)
    try:
        application.resize(1200, 840)
        application.screenshot(output / "wall.png")
        application.point(145, 275)
        application.click(120)
        application.resize(1200, 1000)
        wait_for(lambda: evaluate("!!document.querySelector('.vditor-ir .vditor-reset')"), "Editor did not load")
        browser("eval", "document.activeElement.blur()")
        browser("screenshot", str(output / "editor.png"))
        browser("click", "[data-type=edit-mode]")
        browser("click", "[data-mode=sv]")
        wait_for(lambda: evaluate("!!document.querySelector('textarea.vditor-sv')"), "Source mode did not load")
        browser("screenshot", str(output / "source-preview.png"))
        application.click(108)
        wait_for(lambda: visible(application.child(120)), "Did not return to note")
        application.click(108)
        application.click(114)
        wait_for(lambda: evaluate("!document.querySelector('#settings').hidden"), "Settings did not load")
        browser("eval", "document.querySelectorAll('fieldset')[1].scrollIntoView({block:'start'})")
        browser("screenshot", str(output / "settings.png"))
        panel_top = max(0, int(evaluate("document.querySelectorAll('fieldset')[1].getBoundingClientRect().top")) - 10)
        with Image.open(output / "settings.png") as image:
            image.crop((0, panel_top, image.width, image.height)).save(output / "settings.png", optimize=True)
        application.click(119)
    finally:
        application.close()
    print(output, flush=True)


if __name__ == "__main__":
    main()
