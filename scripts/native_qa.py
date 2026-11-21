import argparse
import ctypes
from ctypes import wintypes
from io import BytesIO
import json
import os
from pathlib import Path
import struct
import subprocess
import time

from PIL import Image


user32 = ctypes.WinDLL("user32", use_last_error=True)
gdi32 = ctypes.WinDLL("gdi32", use_last_error=True)
HANDLE = wintypes.HANDLE
LRESULT = ctypes.c_ssize_t
CALLBACK = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)


class GuiThreadInfo(ctypes.Structure):
    _fields_ = [("size", wintypes.DWORD), ("flags", wintypes.DWORD), ("active", HANDLE), ("focus", HANDLE), ("capture", HANDLE), ("menu_owner", HANDLE), ("move_size", HANDLE), ("caret", HANDLE), ("caret_rect", wintypes.RECT)]


def bind(library, name, result, *arguments):
    function = getattr(library, name)
    function.restype = result
    function.argtypes = arguments
    return function


send = bind(user32, "SendMessageW", LRESULT, HANDLE, wintypes.UINT, wintypes.WPARAM, wintypes.LPARAM)
post = bind(user32, "PostMessageW", wintypes.BOOL, HANDLE, wintypes.UINT, wintypes.WPARAM, wintypes.LPARAM)
get_child = bind(user32, "GetDlgItem", HANDLE, HANDLE, ctypes.c_int)
get_rect = bind(user32, "GetWindowRect", wintypes.BOOL, HANDLE, ctypes.POINTER(wintypes.RECT))
get_client = bind(user32, "GetClientRect", wintypes.BOOL, HANDLE, ctypes.POINTER(wintypes.RECT))
get_text = bind(user32, "GetWindowTextW", ctypes.c_int, HANDLE, wintypes.LPWSTR, ctypes.c_int)
set_text = bind(user32, "SetWindowTextW", wintypes.BOOL, HANDLE, wintypes.LPCWSTR)
get_class = bind(user32, "GetClassNameW", ctypes.c_int, HANDLE, wintypes.LPWSTR, ctypes.c_int)
visible = bind(user32, "IsWindowVisible", wintypes.BOOL, HANDLE)
enum_windows = bind(user32, "EnumWindows", wintypes.BOOL, CALLBACK, wintypes.LPARAM)
enum_children = bind(user32, "EnumChildWindows", wintypes.BOOL, HANDLE, CALLBACK, wintypes.LPARAM)
get_pid = bind(user32, "GetWindowThreadProcessId", wintypes.DWORD, HANDLE, ctypes.POINTER(wintypes.DWORD))
get_id = bind(user32, "GetDlgCtrlID", ctypes.c_int, HANDLE)
get_dc = bind(user32, "GetDC", HANDLE, HANDLE)
release_dc = bind(user32, "ReleaseDC", ctypes.c_int, HANDLE, HANDLE)
create_dc = bind(gdi32, "CreateCompatibleDC", HANDLE, HANDLE)
create_bitmap = bind(gdi32, "CreateCompatibleBitmap", HANDLE, HANDLE, ctypes.c_int, ctypes.c_int)
select_object = bind(gdi32, "SelectObject", HANDLE, HANDLE, HANDLE)
delete_object = bind(gdi32, "DeleteObject", wintypes.BOOL, HANDLE)
delete_dc = bind(gdi32, "DeleteDC", wintypes.BOOL, HANDLE)
print_window = bind(user32, "PrintWindow", wintypes.BOOL, HANDLE, HANDLE, wintypes.UINT)
get_bits = bind(gdi32, "GetDIBits", ctypes.c_int, HANDLE, HANDLE, wintypes.UINT, wintypes.UINT, ctypes.c_void_p, ctypes.c_void_p, wintypes.UINT)
move_window = bind(user32, "MoveWindow", wintypes.BOOL, HANDLE, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_int, wintypes.BOOL)
foreground = bind(user32, "SetForegroundWindow", wintypes.BOOL, HANDLE)
foreground_window = bind(user32, "GetForegroundWindow", HANDLE)
attach_input = bind(user32, "AttachThreadInput", wintypes.BOOL, wintypes.DWORD, wintypes.DWORD, wintypes.BOOL)
current_thread = bind(ctypes.WinDLL("kernel32"), "GetCurrentThreadId", wintypes.DWORD)
keyboard_state = bind(user32, "GetKeyboardState", wintypes.BOOL, ctypes.POINTER(wintypes.BYTE))
set_keyboard_state = bind(user32, "SetKeyboardState", wintypes.BOOL, ctypes.POINTER(wintypes.BYTE))
peek_message = bind(user32, "PeekMessageW", wintypes.BOOL, ctypes.POINTER(wintypes.MSG), HANDLE, wintypes.UINT, wintypes.UINT, wintypes.UINT)
key_event = bind(user32, "keybd_event", None, wintypes.BYTE, wintypes.BYTE, wintypes.DWORD, ctypes.c_size_t)
gui_resources = bind(user32, "GetGuiResources", wintypes.DWORD, HANDLE, wintypes.DWORD)
gui_thread_info = bind(user32, "GetGUIThreadInfo", wintypes.BOOL, wintypes.DWORD, ctypes.POINTER(GuiThreadInfo))


def window_text(handle):
    buffer = ctypes.create_unicode_buffer(send(handle, 0x000E, 0, 0) + 1)
    send(handle, 0x000D, len(buffer), ctypes.addressof(buffer))
    return buffer.value


def class_name(handle):
    buffer = ctypes.create_unicode_buffer(256)
    get_class(handle, buffer, len(buffer))
    return buffer.value


def rectangle(handle):
    bounds = wintypes.RECT()
    get_rect(handle, ctypes.byref(bounds))
    return [bounds.left, bounds.top, bounds.right, bounds.bottom]


def windows_for(pid):
    handles = []

    @CALLBACK
    def callback(handle, parameter):
        owner = wintypes.DWORD()
        get_pid(handle, ctypes.byref(owner))
        if owner.value == pid:
            handles.append(handle)
        return True

    enum_windows(callback, 0)
    return handles


def wait_dialog(process, expected):
    deadline = time.monotonic() + 5
    while time.monotonic() < deadline:
        for handle in windows_for(process.pid):
            if class_name(handle) != "#32770" or not visible(handle):
                continue
            texts = []

            @CALLBACK
            def callback(child, parameter):
                texts.append(window_text(child))
                return True

            enum_children(handle, callback, 0)
            if expected not in " ".join(texts):
                raise AssertionError(f"Unexpected dialog: {texts}")
            return handle
        time.sleep(0.05)
    raise AssertionError(f"Expected dialog not found: {expected}")


def dismiss_dialog(process, expected, answer=1):
    handle = wait_dialog(process, expected)
    button = get_child(handle, answer)
    if not button:
        diagnostic = []
        buttons = []

        @CALLBACK
        def callback(child, parameter):
            diagnostic.append((get_id(child), class_name(child), window_text(child)))
            if class_name(child) == "Button":
                buttons.append(child)
            return True

        enum_children(handle, callback, 0)
        if answer == 1 and len(buttons) == 1:
            button = buttons[0]
        else:
            raise AssertionError(f"Dialog button missing: {answer}; handle={handle}; children={diagnostic}")
    post(button, 0x00F5, 0, 0)
    time.sleep(0.15)


def check_clipboard_paste(application, data, run):
    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
    create_window = bind(user32, "CreateWindowExW", HANDLE, wintypes.DWORD, wintypes.LPCWSTR, wintypes.LPCWSTR, wintypes.DWORD, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_int, HANDLE, HANDLE, HANDLE, ctypes.c_void_p)
    destroy_window = bind(user32, "DestroyWindow", wintypes.BOOL, HANDLE)
    open_clipboard = bind(user32, "OpenClipboard", wintypes.BOOL, HANDLE)
    empty_clipboard = bind(user32, "EmptyClipboard", wintypes.BOOL)
    close_clipboard = bind(user32, "CloseClipboard", wintypes.BOOL)
    set_clipboard = bind(user32, "SetClipboardData", HANDLE, wintypes.UINT, HANDLE)
    clipboard_owner = bind(user32, "GetClipboardOwner", HANDLE)
    register_format = bind(user32, "RegisterClipboardFormatW", wintypes.UINT, wintypes.LPCWSTR)
    global_alloc = bind(kernel32, "GlobalAlloc", HANDLE, wintypes.UINT, ctypes.c_size_t)
    global_lock = bind(kernel32, "GlobalLock", ctypes.c_void_p, HANDLE)
    global_unlock = bind(kernel32, "GlobalUnlock", wintypes.BOOL, HANDLE)
    global_free = bind(kernel32, "GlobalFree", HANDLE, HANDLE)
    owner = create_window(0, "STATIC", "clipboard fixture", 0, 0, 0, 1, 1, None, None, None, None)
    assert owner, ctypes.WinError(ctypes.get_last_error())

    def set_bytes(format_id, payload):
        memory = global_alloc(2, len(payload))
        assert memory, ctypes.WinError(ctypes.get_last_error())
        try:
            pointer = global_lock(memory)
            assert pointer, ctypes.WinError(ctypes.get_last_error())
            ctypes.memmove(pointer, payload, len(payload))
            global_unlock(memory)
            assert open_clipboard(owner), ctypes.WinError(ctypes.get_last_error())
            try:
                assert empty_clipboard()
                assert set_clipboard(format_id, memory), ctypes.WinError(ctypes.get_last_error())
                memory = None
            finally:
                close_clipboard()
        finally:
            if memory:
                global_free(memory)

    try:
        editor = application.child(106)
        application.type(106, "")
        set_bytes(13, "剪贴板文字🙂\0".encode("utf-16-le"))
        application.key(0x56, ctrl=True)
        assert window_text(editor) == "剪贴板文字🙂", "Standard paste lost text"
        for format_id, image_type, extension, expected_color in [(8, "BMP", "bmp", (40, 140, 200)), (register_format("PNG"), "PNG", "png", (190, 90, 50))]:
            payload = BytesIO()
            Image.new("RGB", (152, 72), expected_color).save(payload, format=image_type)
            set_bytes(format_id, payload.getvalue()[14:] if image_type == "BMP" else payload.getvalue())
            application.type(106, "")
            application.key(0x56, ctrl=True)
            markdown = window_text(editor).strip()
            assert markdown.startswith("![图片](assets/") and markdown.endswith(f".{extension})"), markdown
            image_path = data / markdown.removeprefix("![图片](").removesuffix(")")
            with Image.open(image_path) as image:
                assert image.size == (152, 72) and image.convert("RGB").getpixel((4, 4)) == expected_color
            application.key(0x53, ctrl=True)
            wall = json.loads((data / "wall.json").read_text(encoding="utf-8"))
            assert markdown in wall["notes"][0]["body"]
    finally:
        if clipboard_owner() == owner and open_clipboard(owner):
            empty_clipboard()
            close_clipboard()
        destroy_window(owner)


class Application:
    def __init__(self, executable, directory):
        environment = dict(os.environ, ESTICKER_DATA_DIR=str(Path(directory).resolve()))
        self.process = subprocess.Popen([str(Path(executable).resolve())], env=environment)
        self.hwnd = None
        deadline = time.monotonic() + 8
        while time.monotonic() < deadline:
            for handle in windows_for(self.process.pid):
                if class_name(handle) == "EStickerWindow":
                    self.hwnd = handle
                    break
            if self.hwnd:
                break
            if self.process.poll() is not None:
                raise AssertionError(f"App exited at startup: {self.process.returncode}")
            time.sleep(0.05)
        if not self.hwnd:
            raise AssertionError("Main window did not appear")
        self.wait()

    def wait(self, seconds=0.15):
        time.sleep(seconds)
        assert self.process.poll() is None, f"App exited: {self.process.returncode}"

    def child(self, identifier):
        handle = get_child(self.hwnd, identifier)
        assert handle, f"Missing control {identifier}"
        return handle

    def click(self, identifier):
        handle = self.child(identifier)
        assert visible(handle), f"Hidden control {identifier}"
        bounds = wintypes.RECT()
        get_client(handle, ctypes.byref(bounds))
        position = (bounds.bottom // 2 << 16) | (bounds.right // 2)
        post(handle, 0x0201, 1, position)
        post(handle, 0x0202, 0, position)
        self.wait()

    def combo_items(self, identifier):
        handle = self.child(identifier)
        count = send(handle, 0x0146, 0, 0)
        values = []
        for index in range(count):
            length = send(handle, 0x0149, index, 0)
            buffer = ctypes.create_unicode_buffer(length + 1)
            send(handle, 0x0148, index, ctypes.addressof(buffer))
            values.append(buffer.value)
        return values

    def select_combo(self, identifier, index):
        handle = self.child(identifier)
        assert send(handle, 0x014E, index, 0) >= 0, f"Cannot select combo item {index}"
        send(self.hwnd, 0x0111, (1 << 16) | identifier, handle)
        self.wait()

    def point(self, xpos, ypos):
        position = (ypos << 16) | xpos
        send(self.hwnd, 0x0201, 1, position)
        send(self.hwnd, 0x0202, 0, position)
        self.wait()

    def type(self, identifier, value):
        handle = self.child(identifier)
        self.click(identifier)
        buffer = ctypes.create_unicode_buffer(value)
        send(handle, 0x00B1, 0, -1)
        send(handle, 0x00C2, 1, ctypes.addressof(buffer))
        self.wait()
        assert window_text(handle) == value, f"Control {identifier} did not retain text"

    def controls(self):
        children = []

        @CALLBACK
        def callback(handle, parameter):
            if visible(handle):
                children.append({"id": get_id(handle), "class": class_name(handle), "text": window_text(handle), "rect": rectangle(handle)})
            return True

        enum_children(self.hwnd, callback, 0)
        return children

    def resize(self, width, height):
        bounds = rectangle(self.hwnd)
        move_window(self.hwnd, bounds[0], bounds[1], width, height, True)
        self.wait()

    def key(self, virtual_key, ctrl=False, shift=False):
        message = wintypes.MSG()
        peek_message(ctypes.byref(message), None, 0, 0, 0)
        owner = wintypes.DWORD()
        thread = get_pid(self.hwnd, ctypes.byref(owner))
        assert attach_input(current_thread(), thread, True), "Could not attach to the test window's input queue"
        saved = (wintypes.BYTE * 256)()
        state = (wintypes.BYTE * 256)()
        keyboard_state(saved)
        state[0x11] = 0x80 if ctrl else 0
        state[0x10] = 0x80 if shift else 0
        state[virtual_key] = 0x80
        try:
            assert set_keyboard_state(state)
            target = self.focus() or self.hwnd
            post(target, 0x0100, virtual_key, 1)
            self.wait(0.25)
            post(target, 0x0101, virtual_key, 0xC0000001)
        finally:
            set_keyboard_state(saved)
            attach_input(current_thread(), thread, False)
        self.wait()

    def focus(self):
        owner = wintypes.DWORD()
        thread = get_pid(self.hwnd, ctypes.byref(owner))
        information = GuiThreadInfo()
        information.size = ctypes.sizeof(information)
        assert gui_thread_info(thread, ctypes.byref(information))
        return information.focus

    def check_layout(self):
        controls = [control for control in self.controls() if control["class"] in ("Button", "Edit")]
        parent = rectangle(self.hwnd)
        for control in controls:
            left, top, right, bottom = control["rect"]
            assert parent[0] <= left < right <= parent[2], f"Horizontal clipping: {control}"
            assert parent[1] <= top < bottom <= parent[3], f"Vertical clipping: {control}"
        for index, first in enumerate(controls):
            for second in controls[index + 1:]:
                left, top, right, bottom = first["rect"]
                other_left, other_top, other_right, other_bottom = second["rect"]
                overlap = min(right, other_right) > max(left, other_left) and min(bottom, other_bottom) > max(top, other_top)
                assert not overlap, f"Overlapping controls: {first} / {second}"

    def check_paper(self, screenshot, color):
        self.screenshot(screenshot)
        image = Image.open(screenshot)
        parent = rectangle(self.hwnd)
        field = rectangle(self.child(105))
        position = (field[2] - parent[0] - 3, field[3] - parent[1] - 3)
        assert image.getpixel(position) == color, f"Paper input has a gray/white block: {image.getpixel(position)} at {position}"

    def screenshot(self, path):
        self.wait(0.3)
        bounds = rectangle(self.hwnd)
        width, height = bounds[2] - bounds[0], bounds[3] - bounds[1]
        source = get_dc(self.hwnd)
        memory = create_dc(source)
        bitmap = create_bitmap(source, width, height)
        previous = select_object(memory, bitmap)
        try:
            assert print_window(self.hwnd, memory, 2), "PrintWindow failed"
            header = ctypes.create_string_buffer(40)
            struct.pack_into("<IiiHHIIiiII", header, 0, 40, width, -height, 1, 32, 0, width * height * 4, 0, 0, 0, 0)
            pixels = ctypes.create_string_buffer(width * height * 4)
            select_object(memory, previous)
            assert get_bits(memory, bitmap, 0, height, pixels, header, 0), "GetDIBits failed"
            Image.frombuffer("RGB", (width, height), pixels, "raw", "BGRX", 0, 1).save(path)
        finally:
            select_object(memory, previous)
            delete_object(bitmap)
            delete_dc(memory)
            release_dc(self.hwnd, source)

    def close(self):
        post(self.hwnd, 0x0010, 0, 0)
        try:
            self.process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            self.process.terminate()
            raise AssertionError("App did not close cleanly")
        assert self.process.returncode == 0, f"Nonzero exit: {self.process.returncode}"


def run_suite(executable, output, test_clipboard=False):
    run = output / (time.strftime("run-%Y%m%d-%H%M%S") + f"-{os.getpid()}")
    run.mkdir(parents=True)
    data = run / "data"
    results = []
    application = None

    def passed(name):
        results.append(name)
        print(f"PASS: {name}", flush=True)
        (run / "results.json").write_text(json.dumps({"executable": str(Path(executable).resolve()), "passed": results}, ensure_ascii=False, indent=2), encoding="utf-8")

    def wall(path=None):
        return json.loads((path or data / "wall.json").read_text(encoding="utf-8"))

    def wall_view():
        deadline = time.monotonic() + 5
        while not visible(application.child(102)) and time.monotonic() < deadline:
            application.wait()
        assert visible(application.child(102)), "Main navigation did not become visible"
        if visible(application.child(103)) and window_text(application.child(103)):
            application.type(103, "")
        for index in range(4):
            if window_text(application.child(102)) == "文章列表":
                return
            application.click(102)
        raise AssertionError("Cannot return to wall")

    def bucket(index):
        wall_view()
        for step in range(index + 1):
            application.click(102)

    def mode(value):
        label = ["\u7f16\u8f91\u6a21\u5f0f", "\u5206\u5c4f\u9884\u89c8", "\u9605\u8bfb\u6a21\u5f0f"][value]
        for attempt in range(4):
            if window_text(application.child(109)) == label:
                assert bool(visible(application.child(106))) == (value != 2)
                return
            application.click(109)
        raise AssertionError(f"Cannot enter preview mode {value}")

    try:
        application = Application(executable, data)
        application.check_layout()
        application.screenshot(run / "01-wall.png")
        application.key(0x46, ctrl=True)
        assert application.focus() == application.child(103), "Ctrl+F did not focus search"
        application.click(114)
        application.check_layout()
        application.screenshot(run / "02-settings.png")
        for control in [115, 115, 115, 116, 140, 144, 145, 145]:
            application.click(control)
        application.click(118)
        preferences = json.loads((data / "settings.json").read_text(encoding="utf-8"))
        assert preferences["background"] == 3 and preferences["scroll_wall"]
        assert not preferences["markdown_shortcuts"] and not preferences["paper_texture"]
        assert preferences["default_paper"] == 2
        passed("all six settings are usable and persisted")
        application.click(114)
        application.click(115)
        application.click(116)
        application.click(119)
        assert json.loads((data / "settings.json").read_text(encoding="utf-8")) == preferences
        passed("cancel settings leaves saved preferences unchanged")

        application.click(101)
        application.type(149, "work, review")
        application.click(107)
        assert visible(application.child(105)), "Empty title was accepted"
        application.type(105, "便签\r\n" * 12)
        application.click(107)
        assert visible(application.child(105)), "Overflowing short text was accepted"
        application.type(105, "原生回归便签")
        application.click(107)
        application.point(320, 240)
        assert wall()["notes"][0]["paper"] == "Mint"
        application.point(320, 260)
        application.check_layout()
        application.type(149, "work, review, saved")
        application.wait(0.8)
        assert wall()["notes"][0]["tags"] == ["work", "review", "saved"]
        application.click(120)
        for attempt in range(5):
            application.click(106)
            assert visible(application.child(106)), "Clicking body exited editor"
        body = "# 回归正文\r\n\r\n正文点击不退出。**粗体**、*斜体*、~~删除线~~和 `代码`。\r\n\r\n- [x] 自动保存\r\n- [ ] 后续事项\r\n"
        application.type(106, body)
        application.wait(0.8)
        assert wall()["notes"][0]["body"] == body.replace("\r\n", "\n")
        passed("empty/overflow validation, default paper, body clicks, tags and autosave")

        application.key(0x1B)
        application.key(0x1B)
        application.click(102)
        tags = application.combo_items(150)
        assert tags == ["所有标签", "review", "saved", "work"], f"Unexpected tag options: {tags}"
        application.select_combo(150, tags.index("work"))
        assert window_text(application.child(150)) == "work", "Tag filter selection was not retained"
        application.select_combo(150, 0)
        for _ in range(3):
            application.click(102)
        assert window_text(application.child(102)) == "文章列表", "Tag-filter reset did not return to the wall"
        passed("tag filter options, selection, reset and wall navigation")
        application.point(320, 260)
        application.click(120)

        for identifier, fragment in [(121, "# "), (122, "## "), (123, "### "), (132, "#### "), (133, "##### "), (134, "###### "), (124, "**中文🙂**"), (125, "*中文🙂*"), (135, "~~中文🙂~~"), (126, "- "), (136, "- [ ] "), (127, "1. "), (128, "> "), (129, "`中文🙂`"), (137, "```\r\n中文🙂\r\n```"), (138, "| 列1 | 列2 |"), (130, "[中文🙂](url)"), (139, "![中文🙂](path)"), (131, "\r\n---\r\n")]:
            application.type(106, "中文🙂")
            send(application.child(106), 0x00B1, 0, -1)
            application.click(identifier)
            actual = window_text(application.child(106))
            assert fragment in actual, f"Toolbar {identifier}: {actual!r}"
        passed("all nineteen Markdown toolbar actions including Unicode selections")

        prefix = "中文🙂\r\n\r\n"
        draft = prefix + "## **第二节**\r\n\r\n正文"
        application.type(106, draft)
        mode(1)
        editor = application.child(106)
        original = rectangle(editor)
        application.click(147)
        assert not visible(application.child(105)), "Focus mode left the short-note editor visible"
        assert rectangle(editor)[1] < original[1], "Focus mode did not expand the editor"
        application.check_layout()
        application.screenshot(run / "03-focus.png")
        application.key(0x1B)
        assert visible(application.child(105)) and rectangle(editor)[1] == original[1]
        mode(2)
        application.click(146)
        menu = next((handle for handle in windows_for(application.process.pid) if class_name(handle) == "#32768" and visible(handle)), None)
        assert menu, "Outline did not open a native menu"
        for virtual_key in (0x28, 0x0D):
            post(menu, 0x0100, virtual_key, 1)
            application.wait()
        assert visible(editor), "Outline did not jump from reading to source"
        caret = send(editor, 0x00B0, 0, 0) & 0xFFFF
        assert caret == len(prefix.encode("utf-16-le")) // 2, f"Wrong outline caret: {caret}"
        assert window_text(editor) == draft, "Outline or focus mode changed the source"
        passed("focus mode expands the editor and outline jumps to the Unicode heading")

        application.type(106, "shortcut")
        send(application.child(106), 0x00B1, 0, -1)
        application.key(0x42, ctrl=True)
        assert window_text(application.child(106)) == "shortcut", "Disabled shortcuts still applied"
        application.key(0x53, ctrl=True)
        assert wall()["notes"][0]["body"] == "shortcut"
        application.click(108)
        application.click(108)
        application.click(114)
        application.click(140)
        application.click(118)
        application.point(320, 260)
        application.click(120)
        for key, expected in [(0x42, "**快捷键🙂**"), (0x49, "*快捷键🙂*"), (0x4B, "[快捷键🙂](url)")]:
            application.type(106, "快捷键🙂")
            send(application.child(106), 0x00B1, 0, -1)
            application.key(key, ctrl=True)
            assert window_text(application.child(106)) == expected
        application.type(106, "甲\r\n乙")
        send(application.child(106), 0x00B1, 0, -1)
        application.key(0x09)
        assert window_text(application.child(106)) == "    甲\r\n    乙"
        application.key(0x5A, ctrl=True)
        assert window_text(application.child(106)) == "甲\r\n乙", "Markdown insertion was not undoable"
        application.key(0x0D, ctrl=True)
        assert visible(application.child(106)), "Ctrl+Enter exited body editor"
        passed("Markdown shortcuts honor the toggle; Ctrl+S and Ctrl+Enter save in place")

        Image.new("RGB", (240, 90), (60, 135, 100)).save(data / "preview.png")
        external_image = run / "待导入照片.png"
        Image.new("RGB", (200, 64), (145, 68, 191)).save(external_image)
        application.type(106, "")
        mode(1)
        application.click(148)
        deadline = time.monotonic() + 5
        dialog = None
        while not dialog and time.monotonic() < deadline:
            dialog = next((handle for handle in windows_for(application.process.pid) if visible(handle) and window_text(handle) == "导入本地图片"), None)
            if not dialog:
                application.wait()
        assert dialog, "Image picker did not open"
        filename_controls = []
        dialog_controls = []

        @CALLBACK
        def find_filename(handle, parameter):
            control_class = class_name(handle)
            dialog_controls.append((control_class, get_id(handle), bool(visible(handle))))
            if control_class == "Edit" and visible(handle):
                filename_controls.append(handle)
            return True

        while not filename_controls and time.monotonic() < deadline:
            dialog_controls.clear()
            enum_children(dialog, find_filename, 0)
            if not filename_controls:
                application.wait()
        assert len(filename_controls) == 1, f"Unexpected filename controls: {dialog_controls}"
        filename = ctypes.create_unicode_buffer(str(external_image.resolve()))
        send(filename_controls[0], 0x000C, 0, ctypes.addressof(filename))
        post(get_child(dialog, 1), 0x00F5, 0, 0)
        application.wait(1)
        imported = window_text(application.child(106)).strip()
        assert imported.startswith("![图片](assets/") and imported.endswith(".png)"), imported
        image_reference = imported.removeprefix("![图片](").removesuffix(")")
        assert (data / image_reference).read_bytes() == external_image.read_bytes()
        application.key(0x53, ctrl=True)
        assert any(image_reference in note["body"] for note in wall()["notes"]), "Image reference was not autosaved"
        mode(2)
        application.screenshot(run / "06-imported-image.png")
        imported_colors = dict((color, count) for count, color in Image.open(run / "06-imported-image.png").getcolors(2_000_000))
        assert imported_colors.get((145, 68, 191), 0) >= 10_000, "Imported image did not render"
        mode(1)
        passed("native image import copies bytes, saves a portable reference and renders the image")
        if test_clipboard:
            check_clipboard_paste(application, data, run)
            passed("clipboard PNG and DIB paste persist image assets while Unicode text still pastes")
        rich = "# 原生 Markdown 预览\r\n\r\n**粗体**、*斜体*、~~删除线~~、`代码`、[链接](https://example.com)。\r\n\r\n| 项目 | 状态 |\r\n| --- | --- |\r\n| 自动保存 | 已通过 |\r\n\r\n![本地图片](preview.png)\r\n\r\n> 引用内容\r\n\r\n3. 第三项\r\n4. 第四项\r\n\r\n```rust\r\nlet count = 42;\r\n```\r\n"
        application.type(106, rich)
        mode(1)
        application.check_layout()
        application.screenshot(run / "03-split.png")
        application.resize(780, 700)
        application.check_layout()
        application.screenshot(run / "04-split-min.png")
        mode(2)
        application.check_layout()
        application.screenshot(run / "05-reading-min.png")
        application.resize(1040, 900)
        application.check_paper(run / "06-reading-rich.png", (211, 231, 196))
        colors = dict((color, count) for count, color in Image.open(run / "06-reading-rich.png").getcolors(2_000_000))
        assert colors.get((60, 135, 100), 0) >= 10_000, "Local image was not rendered"
        assert colors.get((232, 239, 232), 0) >= 10_000, "Native table header was not rendered"
        send(application.hwnd, 0x020A, ((-360) & 0xFFFF) << 16, 0)
        application.screenshot(run / "06-code-highlight.png")
        code_image = Image.open(run / "06-code-highlight.png").convert("RGB")
        code_colors = dict((color, count) for count, color in code_image.getcolors(2_000_000) if count > 1)
        code_region = code_image.crop((160, 400, 880, 710))
        region_colors = {color for count, color in code_region.getcolors(2_000_000) if count > 1}
        syntax_colors = region_colors - {(247, 244, 237), (255, 254, 250), (237, 232, 222), (57, 54, 47)}
        assert len(syntax_colors) > 12, f"Fenced Rust code preview lacks syntax-colored tokens; palette={sorted(syntax_colors)}"
        application.click(112)
        application.check_layout()
        application.screenshot(run / "07-maximized.png")
        application.click(112)
        application.resize(1040, 740)
        mode(0)
        application.type(106, rich + "\r\n关闭前立即保存🙂")
        application.close()
        assert wall()["notes"][0]["body"].endswith("关闭前立即保存🙂")
        application = Application(executable, data)
        application.point(320, 260)
        application.click(120)
        assert window_text(application.child(106)).endswith("关闭前立即保存🙂")
        application.key(0x1B)
        assert visible(application.child(120)) and not visible(application.child(106))
        application.key(0x1B)
        passed("three preview modes, resize/maximize, close-flush, reopen and Escape navigation")

        application.key(0x4E, ctrl=True)
        application.type(105, "第二张便签")
        application.key(0x0D, ctrl=True)
        assert not visible(application.child(105))
        application.key(0x1B)
        assert window_text(application.child(105)) == "第二张便签"
        application.click(107)
        application.point(320, 240)
        before_drag = wall()["notes"][1]["at"]
        send(application.hwnd, 0x0201, 1, (260 << 16) | 320)
        send(application.hwnd, 0x0200, 1, (340 << 16) | 500)
        send(application.hwnd, 0x0202, 0, (340 << 16) | 500)
        application.wait()
        assert wall()["notes"][1]["at"] != before_drag
        original_layout = [(note["id"], note["at"]) for note in wall()["notes"]]
        bucket(0)
        for option in [1, 2, 0]:
            send(application.child(104), 0x014E, option, 0)
            post(application.hwnd, 0x0111, 104 | (1 << 16), application.child(104))
            application.wait()
            assert send(application.child(104), 0x0147, 0, 0) == option
            application.point(300, 180)
            assert window_text(application.child(105)) == ("原生回归便签" if option == 2 else "第二张便签")
            application.click(108)
        assert [(note["id"], note["at"]) for note in wall()["notes"]] == original_layout
        application.select_combo(150, application.combo_items(150).index("work"))
        application.resize(780, 700)
        application.check_layout()
        application.screenshot(run / "08-tag-filter-min.png")
        application.point(300, 180)
        assert window_text(application.child(105)) == "原生回归便签", "Tag filter opened an unrelated row"
        application.check_layout()
        application.screenshot(run / "08-tag-editor-min.png")
        application.click(108)
        application.type(103, "第二张便签")
        application.point(300, 180)
        assert not visible(application.child(105)), "Search and tag filter were not intersected"
        application.click(102)
        assert window_text(application.child(103)) == "" and send(application.child(150), 0x0147, 0, 0) == 0
        application.resize(1040, 900)
        assert [(note["id"], note["at"]) for note in wall()["notes"]] == original_layout
        passed("tag filtering opens only matching notes, intersects search and clears without changing wall layout")
        application.type(103, "关闭前立即保存")
        application.point(300, 180)
        assert window_text(application.child(105)) == "原生回归便签"
        application.type(105, "归档前的修改")
        application.click(141)
        assert wall()["notes"][0]["pinned"] and wall()["notes"][0]["title"] == "归档前的修改"
        assert "取消置顶" in window_text(application.child(141))
        application.click(142)
        assert wall()["notes"][0]["archived"]
        wall_view()
        application.point(320, 260)
        assert not visible(application.child(105)), "Archived note is still clickable on wall"
        application.screenshot(run / "08-archive-hidden.png")
        bucket(1)
        application.point(300, 180)
        assert window_text(application.child(105)) == "归档前的修改"
        application.click(142)
        assert not wall()["notes"][0]["archived"]
        wall_view()
        application.point(320, 260)
        application.click(143)
        assert wall()["notes"][0]["deleted"]
        bucket(2)
        application.point(300, 180)
        application.check_layout()
        application.click(143)
        assert not wall()["notes"][0]["deleted"]
        assert [(note["id"], note["at"]) for note in wall()["notes"]] == original_layout
        passed("placement cancellation, dragging, body search, pin, archive, trash and restore")

        wall_view()
        application.point(320, 260)
        application.click(143)
        bucket(2)
        application.point(300, 180)
        application.click(110)
        dismiss_dialog(application.process, "不可撤销", 7)
        assert len(wall()["notes"]) == 2
        application.click(110)
        dismiss_dialog(application.process, "不可撤销", 6)
        assert len(wall()["notes"]) == 1 and wall()["notes"][0]["id"] == 2
        passed("permanent deletion requires confirmation and affects only the selected note")

        bucket(0)
        application.point(300, 180)
        application.click(120)
        mode(0)
        application.type(106, "![图片](preview.png)\r\n\r\n" + imported)
        application.click(107)
        wall_view()
        application.click(114)
        application.type(117, "relative-path")
        application.click(118)
        dismiss_dialog(application.process, "绝对路径")
        assert visible(application.child(117))
        existing = run / "existing"
        existing.mkdir()
        sentinel = '{"version":2,"notes":[]}'
        (existing / "wall.json").write_text(sentinel, encoding="utf-8")
        application.type(117, str(existing.resolve()))
        application.click(118)
        dismiss_dialog(application.process, "未覆盖")
        assert (existing / "wall.json").read_text(encoding="utf-8") == sentinel
        old_bytes = (data / "wall.json").read_bytes()
        migrated = run / "迁移后的便签"
        (data / "settings.tmp").mkdir()
        application.type(117, str(migrated.resolve()))
        application.click(118)
        dismiss_dialog(application.process, "未能保存设置")
        assert not (migrated / "wall.json").exists(), "Failed settings save left a partial migration"
        assert not (migrated / "preview.png").exists(), "Failed settings save left a partial image migration"
        assert not (migrated / image_reference).exists(), "Failed settings save left a partial imported asset"
        assert (data / "wall.json").read_bytes() == old_bytes
        (data / "settings.tmp").rmdir()
        application.type(117, str(migrated.resolve()))
        application.click(118)
        assert (migrated / "wall.json").read_bytes() == old_bytes
        assert (migrated / "preview.png").read_bytes() == (data / "preview.png").read_bytes()
        assert (migrated / image_reference).read_bytes() == external_image.read_bytes()
        assert (data / "wall.json").read_bytes() == old_bytes
        application.close()
        application = Application(executable, data)
        application.click(114)
        assert os.path.samefile(window_text(application.child(117)), migrated)
        application.resize(780, 700)
        application.check_layout()
        application.screenshot(run / "09-settings-min.png")
        application.click(119)
        bucket(0)
        application.point(300, 180)
        assert window_text(application.child(105)) == "第二张便签"
        application.type(105, "迁移后保存")
        application.click(107)
        assert wall(migrated / "wall.json")["notes"][0]["title"] == "迁移后保存"
        assert (data / "wall.json").read_bytes() == old_bytes
        passed("directory validation, no-overwrite migration, retained backup and restart routing")

        application.point(300, 180)
        before_failure = (migrated / "wall.json").read_bytes()
        (migrated / "wall.tmp").mkdir()
        application.type(149, "重试, Tags")
        dismiss_dialog(application.process, "写入失败")
        assert (migrated / "wall.json").read_bytes() == before_failure
        assert window_text(application.child(149)) == "重试, Tags"
        (migrated / "wall.tmp").rmdir()
        application.key(0x53, ctrl=True)
        assert wall(migrated / "wall.json")["notes"][0]["tags"] == ["重试", "Tags"]
        passed("failed tag autosave retains editor input and Ctrl+S retries in note details")
        application.click(120)
        mode(0)
        before_failure = (migrated / "wall.json").read_bytes()
        (migrated / "wall.tmp").mkdir()
        application.type(106, "写入失败后不能丢失的正文🙂")
        dismiss_dialog(application.process, "写入失败")
        assert (migrated / "wall.json").read_bytes() == before_failure
        assert window_text(application.child(106)) == "写入失败后不能丢失的正文🙂"
        application.check_paper(run / "10-save-error-recovery.png", (211, 231, 196))
        (migrated / "wall.tmp").rmdir()
        application.key(0x53, ctrl=True)
        assert wall(migrated / "wall.json")["notes"][0]["body"] == "写入失败后不能丢失的正文🙂"
        passed("failed writes preserve disk data and editor text; retry succeeds")

        long_body = "# 长文压力测试\r\n\r\n" + ("正文🙂" * 12 + " **粗体** end.\r\n\r\n") * 600 + "末尾标记 END"
        started = time.monotonic()
        application.type(106, long_body)
        application.key(0x53, ctrl=True)
        assert wall(migrated / "wall.json")["notes"][0]["body"] == long_body.replace("\r\n", "\n")
        mode(2)
        assert time.monotonic() - started < 15, "Long body rendering was unresponsive"
        application.screenshot(run / "11-long-body.png")
        before_resources = gui_resources(application.process._handle, 0)
        resource_samples = [before_resources]
        for attempt in range(12):
            application.click(109)
            resource_samples.append(gui_resources(application.process._handle, 0))
        application.wait(0.75)
        after_resources = gui_resources(application.process._handle, 0)
        assert after_resources <= before_resources + 4, f"GDI resource leak: {before_resources} -> {resource_samples}"
        application.click(108)
        application.click(108)
        wall_view()
        positions = [(note["id"], note["at"]) for note in wall(migrated / "wall.json")["notes"]]
        application.screenshot(run / "12-scroll-before.png")
        send(application.hwnd, 0x020A, ((-120) & 0xFFFF) << 16, 0)
        application.screenshot(run / "13-scroll-after.png")
        assert Image.open(run / "12-scroll-before.png").crop((22, 154, 750, 650)).tobytes() != Image.open(run / "13-scroll-after.png").crop((22, 154, 750, 650)).tobytes()
        assert [(note["id"], note["at"]) for note in wall(migrated / "wall.json")["notes"]] == positions
        application.click(114)
        application.click(116)
        application.click(118)
        application.screenshot(run / "14-scroll-disabled.png")
        send(application.hwnd, 0x020A, ((-120) & 0xFFFF) << 16, 0)
        application.screenshot(run / "15-scroll-still-disabled.png")
        assert Image.open(run / "14-scroll-disabled.png").crop((22, 154, 750, 650)).tobytes() == Image.open(run / "15-scroll-still-disabled.png").crop((22, 154, 750, 650)).tobytes()
        passed("long Unicode bodies, preview resource stability and the wall-scroll switch")

        competitor = subprocess.Popen([str(Path(executable).resolve())], env=dict(os.environ, ESTICKER_DATA_DIR=str(data.resolve())))
        dismiss_dialog(competitor, "另一个 E-Sticker")
        assert competitor.wait(timeout=5) == 0
        application.close()
        application = None
        passed("concurrent instances cannot overwrite the same data")
        damaged = run / "damaged"
        damaged.mkdir()
        broken = '{"version":2,"notes":['
        (damaged / "wall.json").write_text(broken, encoding="utf-8")
        rejected = subprocess.Popen([str(Path(executable).resolve())], env=dict(os.environ, ESTICKER_DATA_DIR=str(damaged.resolve())))
        dismiss_dialog(rejected, "原文件未改动")
        assert rejected.wait(timeout=5) == 0
        assert (damaged / "wall.json").read_text(encoding="utf-8") == broken
        passed("corrupt startup data is reported without being overwritten")
        print(f"RESULTS: {run.resolve()}")
    except Exception:
        if application and application.process.poll() is None:
            application.screenshot(run / "failure.png")
            (run / "failure-controls.json").write_text(json.dumps(application.controls(), ensure_ascii=False, indent=2), encoding="utf-8")
        raise
    finally:
        if application and application.process.poll() is None:
            application.close()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--exe", default="target/debug/e-sticker.exe")
    parser.add_argument("--data", default="artifacts/qa/probe-data")
    parser.add_argument("--output", default="artifacts/qa")
    parser.add_argument("--exercise", action="store_true")
    parser.add_argument("--suite", action="store_true")
    parser.add_argument("--test-clipboard", action="store_true", help="Include clipboard paste tests; replaces the desktop clipboard contents")
    parser.add_argument("--probe-errors", action="store_true")
    arguments = parser.parse_args()
    output = Path(arguments.output)
    output.mkdir(parents=True, exist_ok=True)
    if arguments.suite:
        run_suite(arguments.exe, output, arguments.test_clipboard)
        return
    application = Application(arguments.exe, arguments.data)
    try:
        if arguments.probe_errors:
            application.click(114)
            application.type(117, "relative-path")
            application.click(118)
            dismiss_dialog(application.process, "\u7edd\u5bf9\u8def\u5f84")
            print("ERROR DIALOG OK")
            return
        application.screenshot(output / "wall.png")
        application.click(114)
        application.screenshot(output / "settings.png")
        print(json.dumps(application.controls(), ensure_ascii=False, indent=2))
        if arguments.exercise:
            application.check_layout()
            application.click(119)
            application.click(101)
            application.type(105, "Native regression")
            application.check_layout()
            application.click(107)
            application.point(320, 240)
            application.point(320, 260)
            application.check_layout()
            application.screenshot(output / "detail.png")
            application.click(120)
            application.click(106)
            assert visible(application.child(106)), "Body click closed editor"
            application.type(106, "# Regression\r\n\r\nBody click must not close the editor.\r\n\r\n- [x] autosave\r\n- [ ] later\r\n")
            application.wait(1)
            application.check_layout()
            application.screenshot(output / "body.png")
            application.click(109)
            application.check_layout()
            application.screenshot(output / "split.png")
            application.resize(780, 700)
            application.check_layout()
            application.screenshot(output / "split-min.png")
            application.click(109)
            application.check_layout()
            application.screenshot(output / "read-min.png")
            application.click(108)
            application.check_layout()
            application.screenshot(output / "detail-min.png")
            application.click(108)
            application.click(114)
            application.check_layout()
            application.screenshot(output / "settings-min.png")
    except Exception:
        application.screenshot(output / "failure.png")
        raise
    finally:
        application.close()


if __name__ == "__main__":
    main()
