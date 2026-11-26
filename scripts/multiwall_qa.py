import argparse
import ctypes
from ctypes import wintypes
import hashlib
import json
from pathlib import Path
import time

from native_qa import Application, class_name, dismiss_dialog, post, send, user32, visible, window_text, windows_for


def select_menu_item(application, index):
    deadline = time.monotonic() + 5
    popup = None
    while time.monotonic() < deadline:
        popup = next((handle for handle in windows_for(application.process.pid) if class_name(handle) == "#32768" and visible(handle)), None)
        if popup:
            break
        time.sleep(0.05)
    assert popup, "Native popup menu did not appear"
    menu = send(popup, 0x01E1, 0, 0)
    user32.GetMenuItemCount.argtypes = [wintypes.HMENU]
    user32.GetMenuState.argtypes = [wintypes.HMENU, wintypes.UINT, wintypes.UINT]
    user32.GetMenuState.restype = wintypes.UINT
    indices = [position for position in range(user32.GetMenuItemCount(menu)) if not user32.GetMenuState(menu, position, 0x0400) & 0x0803]
    bounds = wintypes.RECT()
    user32.GetMenuItemRect.argtypes = [wintypes.HWND, wintypes.HMENU, wintypes.UINT, ctypes.POINTER(wintypes.RECT)]
    user32.SetThreadDpiAwarenessContext.argtypes = [ctypes.c_void_p]
    user32.SetThreadDpiAwarenessContext.restype = ctypes.c_void_p
    previous_dpi = user32.SetThreadDpiAwarenessContext(ctypes.c_void_p(-4))
    try:
        assert user32.GetMenuItemRect(None, menu, indices[index], ctypes.byref(bounds))
    finally:
        user32.SetThreadDpiAwarenessContext(previous_dpi)
    point = wintypes.POINT((bounds.left + bounds.right) // 2, (bounds.top + bounds.bottom) // 2)
    assert user32.SetPhysicalCursorPos(point.x, point.y)
    time.sleep(0.1)
    user32.mouse_event(0x0002, 0, 0, 0, 0)
    user32.mouse_event(0x0004, 0, 0, 0, 0)
    application.wait()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--exe", default="target/debug/e-sticker.exe")
    parser.add_argument("--output", default="artifacts/multiwall-qa")
    arguments = parser.parse_args()
    executable = Path(arguments.exe).resolve()
    run = Path(arguments.output).resolve() / time.strftime("run-%Y%m%d-%H%M%S")
    data = run / "data"
    data.mkdir(parents=True)
    path = data / "wall.json"
    source = {"version": 2, "notes": [
        {"id": 1, "title": "被遮住的文章", "body": "# needle\n\n通过全文搜索也能找到。", "paper": "Mint", "at": {"x": 80, "y": 100}, "tags": ["剪辑"]},
        {"id": 2, "title": "上层便签", "body": "删除墙时，归档文章也应进入回收站。", "paper": "Pink", "at": {"x": 80, "y": 100}},
        {"id": 3, "title": "默认归属，不需要额外一面墙", "body": "首页既能随手记录，也汇集每个项目。", "paper": "Yellow", "at": {"x": 540, "y": 190}},
    ]}
    for item in source["notes"]:
        item["created"] = 1795219200
        item["edited"] = 1795219200
    original = json.dumps(source, ensure_ascii=False).encode("utf-8")
    path.write_bytes(original)
    application = Application(executable, data)
    original_cursor = wintypes.POINT()
    user32.GetPhysicalCursorPos.argtypes = [ctypes.POINTER(wintypes.POINT)]
    assert user32.GetPhysicalCursorPos(ctypes.byref(original_cursor))
    user32.SetForegroundWindow.argtypes = [wintypes.HWND]
    user32.SetForegroundWindow(application.hwnd)
    results = []

    def wall():
        return json.loads(path.read_text(encoding="utf-8"))

    def note(note_id):
        return next(item for item in wall()["notes"] if item["id"] == note_id)

    def passed(name):
        results.append({"name": name, "passed": True})
        print(f"PASS {name}", flush=True)

    def menu_item(index):
        select_menu_item(application, index)

    def manage(index):
        user32.SetForegroundWindow(application.hwnd)
        application.click(154)
        menu_item(index)

    def create(name):
        manage(0)
        assert visible(application.child(155))
        application.type(155, name)
        application.click(156)
        return next(item["id"] for item in wall()["walls"] if item["name"] == name)

    def select_wall(wall_id):
        index = next((index + 1 for index, item in enumerate(wall()["walls"]) if item["id"] == wall_id), 0)
        application.select_combo(153, index)

    def canvas():
        if visible(application.child(108)):
            application.click(108)
        if window_text(application.child(102)) != "文章列表":
            application.click(102)

    def open_search(query):
        canvas()
        application.type(103, query)
        application.point(420, 184)
        assert visible(application.child(105))

    def move_article(wall_id):
        user32.SetForegroundWindow(application.hwnd)
        application.click(159)
        index = next((index + 1 for index, item in enumerate(wall()["walls"]) if item["id"] == wall_id), 0)
        menu_item(index)

    def drag(note_id, field, delta_x, delta_y):
        at = note(note_id)[field]
        start_x, start_y = 22 + at["x"] + 80, 154 + at["y"] + 60
        start = (start_y << 16) | start_x
        end = ((start_y + delta_y) << 16) | (start_x + delta_x)
        send(application.hwnd, 0x0201, 1, start)
        send(application.hwnd, 0x0200, 1, end)
        send(application.hwnd, 0x0202, 0, end)
        application.wait()

    def check_navigation_layout():
        application.check_layout()
        controls = [control for control in application.controls() if control["class"] in ("Button", "Edit", "ComboBox")]
        for index, first in enumerate(controls):
            for second in controls[index + 1:]:
                left, top, right, bottom = first["rect"]
                other_left, other_top, other_right, other_bottom = second["rect"]
                assert not (min(right, other_right) > max(left, other_left) and min(bottom, other_bottom) > max(top, other_top)), f"Overlapping navigation: {first} / {second}"

    try:
        assert len(application.combo_items(153)) == 1
        assert "首页" in application.combo_items(153)[0]
        manage(0)
        application.type(155, "取消创建")
        application.click(157)
        assert path.read_bytes() == original
        passed("homepage has no separate default wall; cancelling wall creation does not write data")

        project = create("工作")
        assert (data / "wall.v2.bak").read_bytes() == original
        assert [(item["at"], item["title"]) for item in wall()["notes"]] == [(item["at"], item["title"]) for item in source["notes"]]
        manage(1)
        application.type(155, "剪辑项目")
        application.key(0x0D)
        assert wall()["walls"] == [{"id": project, "name": "剪辑项目"}]
        study = create("学习")
        assert len(application.combo_items(153)) == 3
        passed("native create, rename, Enter confirmation and v2 migration preserve old notes and exact backup")

        application.click(101)
        application.type(105, "学习如何讲好一个故事")
        application.type(149, "学习, 灵感")
        application.click(107)
        application.point(420, 270)
        created = wall()["notes"][-1]
        created_id = created["id"]
        assert created["wall_id"] == study
        assert created["wall_at"] != created["at"]
        assert len(wall()["notes"]) == 4
        before_home = created["at"]
        drag(created_id, "wall_at", 80, 70)
        assert note(created_id)["at"] == before_home
        assert note(created_id)["wall_at"] != created["wall_at"]
        before_category = note(created_id)["wall_at"]
        application.screenshot(run / "01-category-wall.png")
        passed("category creation keeps one article and drag changes only the category layout")

        select_wall(0)
        drag(created_id, "at", 35, 25)
        assert note(created_id)["wall_at"] == before_category
        assert note(created_id)["at"] != before_home
        application.screenshot(run / "02-home-collage.png")
        before = wall()
        application.click(102)
        application.select_combo(104, 2)
        application.select_combo(158, 1)
        application.select_combo(150, 0)
        assert wall() == before
        application.type(103, "needle")
        assert send(application.child(158), 0x0147, 0, 0) == 0
        application.screenshot(run / "03-global-search.png")
        application.point(420, 184)
        assert window_text(application.child(105)) == "被遮住的文章"
        passed("homepage drag is independent; sorting and default filtering do not move notes; global search opens obscured article")

        original_position = note(1)["at"]
        move_article(project)
        assert note(1)["wall_id"] == project
        assert note(1)["at"] == original_position
        assert len(wall()["notes"]) == 4
        open_search("needle")
        application.type(105, "剪辑后的隐藏文章")
        application.click(107)
        assert note(1)["title"] == "剪辑后的隐藏文章"
        assert note(1)["at"] == original_position
        passed("manual classification and shared editing keep article identity, content and homepage position")

        open_search("上层便签")
        move_article(project)
        open_search("上层便签")
        application.click(142)
        assert note(2)["archived"]
        canvas()
        select_wall(project)
        before = wall()
        manage(2)
        dismiss_dialog(application.process, "删除分类墙", 7)
        assert wall() == before
        manage(2)
        dismiss_dialog(application.process, "删除分类墙", 6)
        assert all(item["id"] != project for item in wall()["walls"])
        assert note(1)["deleted"] and note(2)["deleted"]
        assert not note(3)["deleted"] and not note(created_id)["deleted"]
        passed("deletion confirmation can cancel; deleting a wall trashes its active and archived articles only")

        application.click(102)
        application.select_combo(160, 2)
        application.type(103, "needle")
        application.screenshot(run / "04-recycle-bin.png")
        application.point(420, 184)
        application.click(143)
        assert note(1)["wall_id"] == 0 and not note(1)["deleted"]
        assert note(1)["at"] == original_position
        assert window_text(application.child(102)) == "文章列表"
        assert send(application.child(153), 0x0147, 0, 0) == 0
        assert len(wall()["walls"]) == 1
        open_search("needle")
        move_article(study)
        assert note(1)["wall_id"] == study
        assert len(wall()["notes"]) == 4
        passed("recycle restore returns to homepage default without recreating wall; restored article can be classified again")

        canvas()
        select_wall(0)
        bounds = note(created_id)["at"]
        point = wintypes.POINT(22 + bounds["x"] + 80, 154 + bounds["y"] + 60)
        user32.ClientToScreen.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.POINT)]
        user32.ClientToScreen(application.hwnd, ctypes.byref(point))
        post(application.hwnd, 0x007B, application.hwnd, (point.y << 16) | point.x)
        application.wait()
        menu_item(1)
        menu_item(0)
        assert note(created_id)["wall_id"] == 0
        passed("native right-click moves an article out of a category without removing its homepage card")

        before = wall()["notes"]
        application.click(101)
        application.type(105, "首页随手记，稍后再整理")
        application.click(107)
        application.point(600, 230)
        assert wall()["notes"][-1]["wall_id"] == 0
        assert wall()["notes"][:-1] == before
        assert len(application.combo_items(153)) == 2
        passed("creating directly on homepage uses implicit default category and does not move existing cards")

        select_wall(study)
        manage(0)
        application.type(155, "学习")
        before = wall()
        application.click(156)
        dismiss_dialog(application.process, "已有同名")
        assert wall() == before
        assert visible(application.child(155))
        application.click(157)
        manage(0)
        application.type(155, "保存重试")
        blocked = data / "wall.tmp"
        blocked.mkdir()
        application.click(156)
        dismiss_dialog(application.process, "写入失败")
        assert wall() == before
        assert window_text(application.child(155)) == "保存重试"
        blocked.rmdir()
        application.click(156)
        assert len(wall()["walls"]) == 2
        passed("duplicate wall names and failed writes retain original data and editable draft; retry succeeds")

        application.resize(780, 700)
        select_wall(0)
        check_navigation_layout()
        application.screenshot(run / "05-minimum-home.png")
        application.click(102)
        check_navigation_layout()
        application.select_combo(158, 2)
        application.point(420, 184)
        assert window_text(application.child(105)) == "剪辑后的隐藏文章"
        application.click(108)
        application.select_combo(158, 1)
        application.point(420, 184)
        assert window_text(application.child(105)) == "学习如何讲好一个故事"
        application.click(108)
        application.select_combo(158, 0)
        application.screenshot(run / "06-minimum-list.png")
        manage_visible = visible(application.child(154))
        assert not manage_visible
        application.click(102)
        manage(0)
        check_navigation_layout()
        application.screenshot(run / "07-minimum-wall-dialog.png")
        application.click(157)
        passed("780x700 home, list filters and wall dialog have no clipped or overlapping native controls")

        canvas()
        for index, title in enumerate(["把零碎的想法留下来", "先完成，再慢慢打磨", "剪辑节奏：留一点呼吸", "今天学到的新方法", "周末试试这个灵感", "读完这一页再休息"]):
            select_wall(study)
            application.click(101)
            application.type(105, title)
            application.click(107)
            application.point(160 + index * 70, 230 + index % 3 * 90)
            newest = wall()["notes"][-1]
            assert 0 <= newest["at"]["x"] < 780 - 44 - 174
            assert 0 <= newest["at"]["y"] < 700 - 154 - 43 - 142
        select_wall(0)
        application.resize(1120, 800)
        application.screenshot(run / "08-home-overview.png")
        before = wall()
        application.close()
        application = Application(executable, data)
        assert wall() == before
        assert len(application.combo_items(153)) == 3
        select_wall(study)
        application.screenshot(run / "09-category-after-restart.png")
        passed("new category notes accumulate on homepage without reshuffling; restart preserves both layouts and category ownership")

    except Exception as error:
        results.append({"name": str(error), "passed": False})
        (run / "controls-failure.json").write_text(json.dumps(application.controls(), ensure_ascii=False, indent=2), encoding="utf-8")
        application.screenshot(run / "failure.png")
        raise
    finally:
        report = {"exe": str(executable), "sha256": hashlib.sha256(executable.read_bytes()).hexdigest(), "data": str(data), "results": results}
        (run / "results.json").write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
        try:
            if application.process.poll() is None:
                application.close()
        finally:
            user32.SetPhysicalCursorPos(original_cursor.x, original_cursor.y)
        print(f"Artifacts: {run}", flush=True)


if __name__ == "__main__":
    main()
