use super::*;

impl App {
    pub(super) unsafe fn refresh_wall_controls(&mut self) {
        if self.current_wall != model::DEFAULT_WALL
            && !self
                .wall
                .walls
                .iter()
                .any(|wall| wall.id == self.current_wall)
        {
            self.current_wall = model::DEFAULT_WALL;
        }
        if self.category_filter.is_some_and(|id| {
            id != model::DEFAULT_WALL && !self.wall.walls.iter().any(|wall| wall.id == id)
        }) {
            self.category_filter = None;
        }
        let picker = self.control(WALL_PICKER);
        SendMessageW(picker, CB_RESETCONTENT, 0, 0);
        let count = self.wall.rows_in("", Sort::Edited, 0).len();
        SendMessageW(
            picker,
            CB_ADDSTRING,
            0,
            wide(&format!("首页 · 全部 {count} 张")).as_ptr() as LPARAM,
        );
        for wall in &self.wall.walls {
            let count = self
                .wall
                .notes
                .iter()
                .filter(|note| note.wall_id == wall.id && !note.deleted && !note.archived)
                .count();
            SendMessageW(
                picker,
                CB_ADDSTRING,
                0,
                wide(&format!("{} · {count} 张", wall.name)).as_ptr() as LPARAM,
            );
        }
        let selected = self
            .wall
            .walls
            .iter()
            .position(|wall| wall.id == self.current_wall)
            .map_or(0, |index| index + 1);
        SendMessageW(picker, CB_SETCURSEL, selected, 0);
        let filter = self.control(CATEGORY_FILTER);
        SendMessageW(filter, CB_RESETCONTENT, 0, 0);
        for name in ["全部分类", "未分类 · 仅首页"] {
            SendMessageW(filter, CB_ADDSTRING, 0, wide(name).as_ptr() as LPARAM);
        }
        for wall in &self.wall.walls {
            SendMessageW(filter, CB_ADDSTRING, 0, wide(&wall.name).as_ptr() as LPARAM);
        }
        let selected = match self.category_filter {
            None => 0,
            Some(model::DEFAULT_WALL) => 1,
            Some(id) => self
                .wall
                .walls
                .iter()
                .position(|wall| wall.id == id)
                .map_or(0, |index| index + 2),
        };
        SendMessageW(filter, CB_SETCURSEL, selected, 0);
    }

    pub(super) unsafe fn switch_wall(&mut self, wall_id: u64) {
        self.wall_offsets.insert(self.current_wall, self.offset);
        self.current_wall = wall_id;
        self.offset = self.wall_offsets.get(&wall_id).copied().unwrap_or_default();
        self.category_filter = (wall_id != model::DEFAULT_WALL).then_some(wall_id);
        self.query.clear();
        self.label(SEARCH, "");
        self.selected_tag = None;
        self.bucket = 0;
        self.list_scroll = 0;
        self.status.clear();
        self.row_hover.set(None);
        self.refresh_tag_filter();
        self.refresh_wall_controls();
        self.clamp_scroll();
        self.layout();
        self.redraw();
    }

    pub(super) unsafe fn return_home(&mut self) {
        self.list = false;
        self.switch_wall(model::DEFAULT_WALL);
        self.status = "已恢复到首页 · 可通过右键重新分类".into();
        self.redraw();
    }

    pub(super) unsafe fn wall_menu(&mut self) {
        let menu = CreatePopupMenu();
        if menu.is_null() {
            return;
        }
        AppendMenuW(menu, MF_STRING, 1, wide("新建分类墙…").as_ptr());
        let flags = MF_STRING
            | if self.current_wall == model::DEFAULT_WALL {
                MF_GRAYED
            } else {
                0
            };
        AppendMenuW(menu, flags, 2, wide("重命名当前墙…").as_ptr());
        AppendMenuW(menu, MF_SEPARATOR, 0, null());
        AppendMenuW(menu, flags, 3, wide("删除当前墙…").as_ptr());
        let mut area = zeroed();
        GetWindowRect(self.control(WALL_MENU), &mut area);
        let command = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_NONOTIFY,
            area.left,
            area.bottom,
            0,
            self.hwnd,
            null(),
        );
        DestroyMenu(menu);
        match command {
            1 => self.begin_wall_dialog(model::DEFAULT_WALL),
            2 if self.current_wall != model::DEFAULT_WALL => {
                self.begin_wall_dialog(self.current_wall)
            }
            3 if self.current_wall != model::DEFAULT_WALL => self.delete_current_wall(),
            _ => {}
        }
    }

    unsafe fn begin_wall_dialog(&mut self, wall_id: u64) {
        self.wall_dialog = Some(wall_id);
        self.label(
            WALL_NAME,
            if wall_id == model::DEFAULT_WALL {
                ""
            } else {
                self.wall.wall_name(wall_id)
            },
        );
        self.label(
            WALL_SAVE,
            if wall_id == model::DEFAULT_WALL {
                "创建墙面"
            } else {
                "保存名称"
            },
        );
        self.status.clear();
        self.layout();
        SetFocus(self.control(WALL_NAME));
        SendMessageW(self.control(WALL_NAME), EM_SETSEL, 0, -1);
        self.redraw();
    }

    pub(super) unsafe fn save_wall_dialog(&mut self) {
        let Some(wall_id) = self.wall_dialog else {
            return;
        };
        let name = self.value(WALL_NAME);
        let mut next = self.wall.clone();
        let result = if wall_id == model::DEFAULT_WALL {
            next.create_wall(&name)
        } else {
            next.rename_wall(wall_id, &name).map(|()| wall_id)
        };
        match result {
            Ok(id) => {
                if self.store_wall(next, "墙面已保存") {
                    self.wall_dialog = None;
                    self.list = false;
                    self.switch_wall(id);
                    self.status = "分类墙已保存 · 新便签将归属这里".into();
                    self.redraw();
                }
            }
            Err(error) => {
                message(self.hwnd, &error.to_string(), MB_ICONWARNING);
            }
        }
    }

    unsafe fn delete_current_wall(&mut self) {
        let wall_id = self.current_wall;
        let count = self
            .wall
            .notes
            .iter()
            .filter(|note| note.wall_id == wall_id && !note.deleted)
            .count();
        let name = self.wall.wall_name(wall_id);
        if message(
            self.hwnd,
            &format!(
                "删除分类墙「{name}」？\n\n其中 {count} 篇文章（包含归档文章）将全部进入回收站，并从首页移除。\n恢复文章后会回到首页默认归属，不会重建此墙。"
            ),
            MB_YESNO | MB_DEFBUTTON2 | MB_ICONWARNING,
        ) != IDYES
        {
            return;
        }
        let mut next = self.wall.clone();
        match next.delete_wall(wall_id) {
            Ok(_) if self.store_wall(next, "分类墙已删除") => {
                self.wall_offsets.remove(&wall_id);
                self.list = false;
                self.switch_wall(model::DEFAULT_WALL);
                self.status = format!("分类墙已删除 · {count} 篇文章已进入回收站");
                self.redraw();
            }
            Err(error) => {
                message(self.hwnd, &error.to_string(), MB_ICONWARNING);
            }
            _ => {}
        }
    }

    pub(super) unsafe fn move_menu(&mut self, note_id: u64, point: Point) {
        let Some(note) = self
            .wall
            .notes
            .iter()
            .find(|note| note.id == note_id && !note.deleted)
        else {
            return;
        };
        let current = note.wall_id;
        let mut targets = vec![model::DEFAULT_WALL];
        targets.extend(self.wall.walls.iter().map(|wall| wall.id));
        let menu = CreatePopupMenu();
        if menu.is_null() {
            return;
        }
        for (index, id) in targets.iter().enumerate() {
            let name = if *id == model::DEFAULT_WALL {
                "未分类 · 仅首页"
            } else {
                self.wall.wall_name(*id)
            };
            let flags = MF_STRING | if *id == current { MF_CHECKED } else { 0 };
            AppendMenuW(
                menu,
                flags,
                index + 1,
                wide(&name.replace('&', "&&")).as_ptr(),
            );
        }
        let selected = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_NONOTIFY,
            point.x,
            point.y,
            0,
            self.hwnd,
            null(),
        );
        DestroyMenu(menu);
        if selected <= 0 {
            return;
        }
        let Some(target) = targets.get(selected as usize - 1).copied() else {
            return;
        };
        let mut next = self.wall.clone();
        match next.assign_wall(note_id, target) {
            Ok(()) => {
                let notice = format!("已移动到「{}」· 首页位置不变", next.wall_name(target));
                if self.store_wall(next, &notice) {
                    if self.editing.is_some() {
                        self.leave_edit();
                    }
                    self.clamp_scroll();
                    self.layout();
                    self.redraw();
                }
            }
            Err(error) => {
                message(self.hwnd, &error.to_string(), MB_ICONWARNING);
            }
        }
    }

    pub(super) unsafe fn note_context(&mut self, screen: Point) {
        if self.settings.is_some()
            || self.editing.is_some()
            || self.placement.is_some()
            || self.wall_dialog.is_some()
        {
            return;
        }
        let mut point = POINT {
            x: screen.x,
            y: screen.y,
        };
        if screen.x == -1 && screen.y == -1 {
            GetCursorPos(&mut point);
        }
        let screen = Point {
            x: point.x,
            y: point.y,
        };
        ScreenToClient(self.hwnd, &mut point);
        let point = Point {
            x: point.x,
            y: point.y,
        };
        let note = if self.view_list() {
            self.list_hit(point)
        } else {
            self.wall.hit_in(self.wall_point(point), self.current_wall)
        };
        let Some(note) = note else {
            return;
        };
        let id = note.id;
        let deleted = note.deleted;
        let menu = CreatePopupMenu();
        if menu.is_null() {
            return;
        }
        AppendMenuW(menu, MF_STRING, 1, wide("打开文章").as_ptr());
        AppendMenuW(
            menu,
            MF_STRING,
            2,
            wide(if deleted {
                "恢复到首页"
            } else {
                "移动到分类…"
            })
            .as_ptr(),
        );
        if !deleted {
            AppendMenuW(menu, MF_STRING, 3, wide("移入回收站").as_ptr());
        }
        let selected = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_NONOTIFY,
            screen.x,
            screen.y,
            0,
            self.hwnd,
            null(),
        );
        DestroyMenu(menu);
        match selected {
            1 => {
                if let Some(note) = self.wall.notes.iter().find(|note| note.id == id) {
                    self.begin_edit(Some(id), note.draft());
                }
            }
            2 if deleted => {
                if self.commit(|wall| wall.set_deleted(id, false), "已恢复到首页") {
                    self.return_home();
                }
            }
            2 => self.move_menu(id, screen),
            3 if !deleted => {
                self.commit(|wall| wall.set_deleted(id, true), "文章已移入回收站");
                self.clamp_scroll();
            }
            _ => {}
        }
    }

    pub(super) unsafe fn note_paper(&self, dc: HDC, note: &model::Note, at: Point, flying: bool) {
        let saved = SaveDC(dc);
        if self.current_wall == model::DEFAULT_WALL && note.home_tilt != 0 {
            let angle = (note.home_tilt as f32).to_radians();
            let cosine = angle.cos();
            let sine = angle.sin();
            let center_x = (EDGE + at.x - self.offset.x) as f32 + note.width as f32 / 2.0;
            let center_y = (TOP + at.y - self.offset.y) as f32 + note.height as f32 / 2.0;
            SetGraphicsMode(dc, GM_ADVANCED);
            SetWorldTransform(
                dc,
                &XFORM {
                    eM11: cosine,
                    eM12: sine,
                    eM21: -sine,
                    eM22: cosine,
                    eDx: center_x * (1.0 - cosine) + center_y * sine,
                    eDy: center_y * (1.0 - cosine) - center_x * sine,
                },
            );
        }
        self.paper(
            dc,
            at,
            &note.title,
            note.paper,
            flying,
            Point {
                x: note.width,
                y: note.height,
            },
        );
        RestoreDC(dc, saved);
    }

    pub(super) unsafe fn draw_wall_dialog(&self, dc: HDC) {
        let area = self.editor_area();
        let left = area.left + 32;
        let right = area.right - 32;
        rounded(
            dc,
            rect(area.left, TOP, area.right, TOP + 240),
            PANEL_RADIUS,
            WHITE,
            BORDER,
        );
        text(
            dc,
            self.fonts[4],
            if self.wall_dialog == Some(model::DEFAULT_WALL) {
                "新建分类墙"
            } else {
                "重命名分类墙"
            },
            rect(left, TOP + 24, right, TOP + 55),
            INK,
            DT_SINGLELINE,
        );
        text(
            dc,
            self.fonts[0],
            "每篇文章仅归属一面墙，也会汇集到首页。",
            rect(left, TOP + 64, right, TOP + 90),
            SECONDARY,
            DT_SINGLELINE,
        );
        rounded(
            dc,
            rect(left - 6, TOP + 100, right + 6, TOP + 146),
            BUTTON_RADIUS,
            WHITE,
            BORDER,
        );
    }
}
