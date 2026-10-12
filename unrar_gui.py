"""UnRAR — WinRAR-style archive manager GUI (Tkinter, stdlib only).

Layout mirrors WinRAR: menu bar, icon toolbar, location bar,
columned file list with icons, and a two-pane status bar.
"""

from __future__ import annotations

import json
import os
import queue
import stat
import sys
import tempfile
import threading
import tkinter as tk
from datetime import datetime
from pathlib import Path
from tkinter import filedialog, messagebox, simpledialog, ttk

import unrar_engine as eng
import unrar_icons

APP_TITLE = "UnRAR"
APP_VERSION = "1.0"
LEVELS = ("Store", "Fastest", "Fast", "Normal", "Good", "Best")

CFG_DIR = Path(os.environ.get("APPDATA") or Path.home()) / "UnRAR"
CFG_FILE = CFG_DIR / "unrar.json"
MAX_RECENT = 8


def fmt_size(n: float) -> str:
    for unit in ("B", "KB", "MB", "GB", "TB"):
        if n < 1024 or unit == "TB":
            return f"{n:,.0f} {unit}" if unit == "B" else f"{n:,.1f} {unit}"
        n /= 1024
    return str(n)


def fmt_time(dt) -> str:
    return dt.strftime("%Y-%m-%d %H:%M") if dt else ""


def _load_cfg() -> dict:
    try:
        return json.loads(CFG_FILE.read_text(encoding="utf-8"))
    except Exception:
        return {}


def _save_cfg(cfg: dict):
    try:
        CFG_DIR.mkdir(parents=True, exist_ok=True)
        CFG_FILE.write_text(json.dumps(cfg, indent=1), encoding="utf-8")
    except OSError:
        pass


class UnRARApp(tk.Tk):
    def __init__(self, archive: str | None = None):
        super().__init__()
        self.title(APP_TITLE)
        self.geometry("1000x580")
        self.minsize(680, 340)

        self.archive_path: str | None = None
        self.info: eng.ArchiveInfo | None = None
        self._q: queue.Queue = queue.Queue()
        self._cfg = _load_cfg()
        self._level = tk.StringVar(value=self._cfg.get("level", "Normal"))
        self._recent: list[str] = [p for p in self._cfg.get("recent", [])
                                   if os.path.exists(p)]

        # native-ish theme
        style = ttk.Style(self)
        for theme in ("xpnative", "vista", "winnative", "clam"):
            if theme in style.theme_names():
                style.theme_use(theme)
                break
        style.configure("Toolbar.TButton", padding=(6, 2))
        style.configure("Treeview", rowheight=20)
        style.configure("Status.TLabel", padding=(8, 2))

        self._icons = unrar_icons.load(self)
        self._icons32 = unrar_icons.load_toolbar(self)
        try:
            self.iconphoto(True, self._icons32["folder"])
        except tk.TclError:
            pass

        self._build_menu()
        self._build_toolbar()
        self._build_location_bar()
        self._build_list()
        self._build_findbar()
        self._build_statusbar()
        self._build_context_menu()
        self.after(80, self._poll_queue)

        if archive:
            self.open_archive(archive)
        else:
            self._set_status("Ready — open an archive (Ctrl+O) or create one "
                             "(Ctrl+N).")
            self._update_stats()

    # ---------------- UI construction ----------------

    def _build_menu(self):
        menubar = tk.Menu(self)

        m_file = tk.Menu(menubar, tearoff=0)
        m_file.add_command(label="&Open archive…\tCtrl+O",
                           command=self.cmd_open)
        m_file.add_command(label="&New ZIP archive…\tCtrl+N",
                           command=self.cmd_new)
        m_file.add_separator()
        self._recent_menu = tk.Menu(m_file, tearoff=0)
        m_file.add_cascade(label="&Recent archives", menu=self._recent_menu)
        self._refresh_recent_menu()
        m_file.add_separator()
        m_file.add_command(label="E&xit\tAlt+F4", command=self.destroy)
        menubar.add_cascade(label="&File", menu=m_file)

        m_cmd = tk.Menu(menubar, tearoff=0)
        m_cmd.add_command(label="&Add files…", command=self.cmd_add)
        m_cmd.add_command(label="&Extract to…\tCtrl+E", command=self.cmd_extract)
        m_cmd.add_command(label="&Test archive\tCtrl+T", command=self.cmd_test)
        m_cmd.add_command(label="&View file\tEnter", command=self.cmd_view)
        m_cmd.add_command(label="&Delete\tDel", command=self.cmd_delete)
        m_cmd.add_separator()
        m_cmd.add_command(label="&Find…\tCtrl+F", command=self.cmd_find)
        m_cmd.add_command(label="&Comment…", command=self.cmd_comment)
        m_cmd.add_command(label="&Lock/Unlock archive", command=self.cmd_lock)
        m_cmd.add_separator()
        m_cmd.add_command(label="&Archive information\tCtrl+I",
                          command=self.cmd_info)
        menubar.add_cascade(label="&Commands", menu=m_cmd)

        m_opt = tk.Menu(menubar, tearoff=0)
        m_lvl = tk.Menu(m_opt, tearoff=0)
        for lv in LEVELS:
            m_lvl.add_radiobutton(label=lv, variable=self._level, value=lv,
                                  command=self._save_level)
        m_opt.add_cascade(label="Default &compression level", menu=m_lvl)
        menubar.add_cascade(label="&Options", menu=m_opt)

        m_help = tk.Menu(menubar, tearoff=0)
        m_help.add_command(label="&About UnRAR…", command=self.cmd_about)
        menubar.add_cascade(label="&Help", menu=m_help)
        self.config(menu=menubar)

        self.bind("<Control-o>", lambda e: self.cmd_open())
        self.bind("<Control-n>", lambda e: self.cmd_new())
        self.bind("<Control-e>", lambda e: self.cmd_extract())
        self.bind("<Control-t>", lambda e: self.cmd_test())
        self.bind("<Control-i>", lambda e: self.cmd_info())
        self.bind("<Control-f>", lambda e: self.cmd_find())
        self.bind("<Control-a>", lambda e: self._select_all())
        self.bind("<Return>", lambda e: self.cmd_view())
        self.bind("<Delete>", lambda e: self.cmd_delete())
        self.bind("<BackSpace>", lambda e: self._clear_filter())

    def _build_toolbar(self):
        bar = ttk.Frame(self)
        bar.pack(side=tk.TOP, fill=tk.X)
        self._toolbar = bar
        items = [
            ("open", "Open", self.cmd_open),
            ("new", "New", self.cmd_new),
            None,
            ("add", "Add", self.cmd_add),
            ("extract", "Extract to", self.cmd_extract),
            ("test", "Test", self.cmd_test),
            ("view", "View", self.cmd_view),
            ("delete", "Delete", self.cmd_delete),
            None,
            ("find", "Find", self.cmd_find),
            ("comment", "Comment", self.cmd_comment),
            ("lock", "Lock", self.cmd_lock),
            ("info", "Info", self.cmd_info),
            None,
            ("help", "About", self.cmd_about),
        ]
        for it in items:
            if it is None:
                ttk.Separator(bar, orient=tk.VERTICAL).pack(
                    side=tk.LEFT, fill=tk.Y, padx=4, pady=3)
                continue
            icon, text, cmd = it
            ttk.Button(bar, image=self._icons32[icon], text=text,
                       compound=tk.TOP, style="Toolbar.TButton",
                       command=cmd).pack(side=tk.LEFT, padx=1, pady=1)

    def _build_location_bar(self):
        bar = ttk.Frame(self, padding=(4, 2))
        bar.pack(side=tk.TOP, fill=tk.X)
        ttk.Button(bar, image=self._icons["up"], width=3,
                   command=self._go_up).pack(side=tk.LEFT)
        self._locvar = tk.StringVar(value="No archive open")
        loc = ttk.Entry(bar, textvariable=self._locvar, state="readonly")
        loc.pack(side=tk.LEFT, fill=tk.X, expand=True, padx=(4, 0))

    def _build_list(self):
        frame = ttk.Frame(self)
        frame.pack(fill=tk.BOTH, expand=True)
        cols = ("size", "packed", "type", "modified", "crc", "ratio", "method")
        self.tree = ttk.Treeview(frame, columns=cols, show="tree headings",
                                 selectmode="extended")
        self.tree.heading("#0", text="Name",
                          command=lambda: self._sort_by("#0"))
        self.tree.column("#0", width=280, anchor=tk.W)
        heads = {
            "size": ("Size", 80), "packed": ("Packed", 80),
            "type": ("Type", 80), "modified": ("Modified", 125),
            "crc": ("CRC32", 80), "ratio": ("Ratio", 55),
            "method": ("Method", 110),
        }
        for c, (label, w) in heads.items():
            self.tree.heading(c, text=label,
                              command=lambda c=c: self._sort_by(c))
            anchor = tk.W if c in ("type", "method") else tk.E
            self.tree.column(c, width=w, anchor=anchor)
        vsb = ttk.Scrollbar(frame, orient=tk.VERTICAL,
                            command=self.tree.yview)
        self.tree.configure(yscrollcommand=vsb.set)
        vsb.pack(side=tk.RIGHT, fill=tk.Y)
        self.tree.pack(side=tk.LEFT, fill=tk.BOTH, expand=True)
        self.tree.bind("<Double-1>", lambda e: self.cmd_view())
        self.tree.bind("<Button-3>", self._popup)
        self.tree.bind("<<TreeviewSelect>>", lambda e: self._update_stats())

    def _build_findbar(self):
        self._findbar = ttk.Frame(self, padding=(4, 2))
        ttk.Label(self._findbar, text="Find:").pack(side=tk.LEFT)
        self._findvar = tk.StringVar()
        ent = ttk.Entry(self._findbar, textvariable=self._findvar)
        ent.pack(side=tk.LEFT, fill=tk.X, expand=True, padx=4)
        ent.bind("<KeyRelease>", lambda e: self._apply_filter())
        ent.bind("<Escape>", lambda e: self._clear_filter())
        ttk.Button(self._findbar, text="x", width=3,
                   command=self._clear_filter).pack(side=tk.LEFT)

    def _build_statusbar(self):
        bar = ttk.Frame(self, relief=tk.SUNKEN)
        bar.pack(side=tk.BOTTOM, fill=tk.X)
        self._stat_left = ttk.Label(bar, text="", style="Status.TLabel")
        self._stat_left.pack(side=tk.LEFT, fill=tk.X, expand=True)
        self._stat_right = ttk.Label(bar, text="", style="Status.TLabel")
        self._stat_right.pack(side=tk.RIGHT)

    def _build_context_menu(self):
        self._ctx = tk.Menu(self, tearoff=0)
        self._ctx.add_command(label="View", command=self.cmd_view)
        self._ctx.add_command(label="Extract to…", command=self.cmd_extract)
        self._ctx.add_separator()
        self._ctx.add_command(label="Select all", command=self._select_all)
        self._ctx.add_command(label="Delete", command=self.cmd_delete)
        self._ctx.add_separator()
        self._ctx.add_command(label="Archive info", command=self.cmd_info)

    # ---------------- helpers ----------------

    def _save_level(self):
        self._cfg["level"] = self._level.get()
        _save_cfg(self._cfg)

    def _push_recent(self, path: str):
        self._recent = [path] + [p for p in self._recent if p != path]
        self._recent = self._recent[:MAX_RECENT]
        self._cfg["recent"] = self._recent
        _save_cfg(self._cfg)
        self._refresh_recent_menu()

    def _refresh_recent_menu(self):
        self._recent_menu.delete(0, tk.END)
        if not self._recent:
            self._recent_menu.add_command(label="(empty)", state=tk.DISABLED)
            return
        for p in self._recent:
            self._recent_menu.add_command(
                label=p, command=lambda p=p: self.open_archive(p))

    def _popup(self, event):
        iid = self.tree.identify_row(event.y)
        if iid and iid not in self.tree.selection():
            self.tree.selection_set(iid)
        self._ctx.tk_popup(event.x_root, event.y_root)

    def _set_status(self, text: str):
        self._stat_left.config(text=text)

    def _update_stats(self):
        if not self.info:
            self._stat_right.config(text="")
            return
        sel = self.tree.selection()
        sel_bytes = 0
        names = {e.name: e for e in self.info.entries}
        for iid in sel:
            e = names.get(self.tree.item(iid, "text"))
            if e and not e.is_dir:
                sel_bytes += e.size
        right = self.info.format.upper()
        if eng.is_encrypted(self.info):
            right += " | encrypted"
        total = sum(e.size for e in self.info.entries)
        left = f"{len(self.info.entries)} objects — {fmt_size(total)}"
        if sel:
            left += f"   |   {len(sel)} selected — {fmt_size(sel_bytes)}"
        self._stat_left.config(text=left)
        self._stat_right.config(text=right)

    def _sort_by(self, col):
        def key(iid):
            if col == "#0":
                return self.tree.item(iid, "text").lower()
            v = self.tree.set(iid, col)
            try:
                return (0, float(v.replace(",", "").replace("%", "")
                                   .split()[0] or 0))
            except (ValueError, IndexError):
                return (1, v)
        for pos, iid in enumerate(sorted(self.tree.get_children(""), key=key)):
            self.tree.move(iid, "", pos)

    def _selected_names(self) -> list[str]:
        return [self.tree.item(iid, "text") for iid in self.tree.selection()]

    def _select_all(self):
        self.tree.selection_set(self.tree.get_children())

    def _go_up(self):
        # Open the folder that contains the archive — cheap "up" action.
        if self.archive_path:
            os.startfile(os.path.dirname(os.path.abspath(
                self.archive_path)))  # noqa: S606

    # ---- find bar ----

    def cmd_find(self):
        self._findbar.pack(side=tk.BOTTOM, fill=tk.X,
                           before=self._stat_left.master)
        self._findbar.winfo_children()[1].focus_set()

    def _clear_filter(self):
        self._findvar.set("")
        self._findbar.pack_forget()
        self._populate()

    def _apply_filter(self):
        needle = self._findvar.get().lower()
        self._populate(needle)

    # ---- background worker ----

    def _run_bg(self, fn, done_msg: str):
        def work():
            try:
                result = fn()
            except Exception as e:              # noqa: BLE001 -> shown to user
                self._q.put(("error", str(e)))
            else:
                self._q.put(("done", (result, done_msg)))
        self._set_status("Working…")
        self.configure(cursor="watch")
        threading.Thread(target=work, daemon=True).start()

    def _poll_queue(self):
        try:
            kind, payload = self._q.get_nowait()
        except queue.Empty:
            pass
        else:
            self.configure(cursor="")
            if kind == "error":
                self._set_status("Error")
                messagebox.showerror(APP_TITLE, payload)
            else:
                result, done_msg = payload
                self._set_status(done_msg.format(result=result))
                if self.archive_path:
                    self._reload()
        self.after(80, self._poll_queue)

    def _reload(self):
        if self.archive_path and os.path.exists(self.archive_path):
            self.open_archive(self.archive_path, silent=True)

    # ---- list rendering ----

    def _entry_icon(self, e: eng.Entry):
        return self._icons["folder" if e.is_dir else "file"]

    def _populate(self, needle: str = ""):
        self.tree.delete(*self.tree.get_children())
        if not self.info:
            return
        for e in self.info.entries:
            if needle and needle not in e.name.lower():
                continue
            ext = Path(e.name).suffix.lstrip(".").upper() + " file" \
                if not e.is_dir else "Folder"
            crc = f"{e.crc:08X}" if e.crc is not None else ""
            self.tree.insert("", tk.END, text=e.name,
                             image=self._entry_icon(e),
                             values=("" if e.is_dir else fmt_size(e.size),
                                     "" if e.is_dir else fmt_size(e.packed),
                                     ext, fmt_time(e.mtime), crc,
                                     e.ratio, e.method))
        self._update_stats()

    # ---------------- commands ----------------

    def cmd_open(self):
        path = filedialog.askopenfilename(
            title="Open archive",
            filetypes=[("Archives",
                        "*.rar *.rev *.r00 *.zip *.zipx *.jar *.7z *.tar "
                        "*.gz *.bz2 *.xz *.zst *.cab *.tgz *.tbz2 *.txz"),
                       ("All files", "*.*")])
        if path:
            self.open_archive(path)

    def open_archive(self, path: str, silent: bool = False):
        try:
            info = eng.list_archive(path)
        except Exception as e:                  # noqa: BLE001
            if not silent:
                messagebox.showerror(APP_TITLE, str(e))
            self._set_status(f"Cannot open: {e}")
            return
        self.archive_path = path
        self.info = info
        self.title(f"{APP_TITLE} - {path}")
        self._locvar.set(path)
        self._push_recent(path)
        self._clear_filter()
        self._populate()
        note = ""
        if info.needs_backend:
            note = f"  [{info.needs_backend} contents need an external tool]"
        self._set_status(f"{info.format.upper()} archive opened.{note}")

    def cmd_new(self):
        path = filedialog.asksaveasfilename(
            title="New archive", defaultextension=".zip",
            filetypes=[("ZIP archive", "*.zip")])
        if not path:
            return
        self.archive_path = path
        self.info = eng.ArchiveInfo(path=path, format="zip")
        self.title(f"{APP_TITLE} - {path}")
        self._locvar.set(path)
        self._populate()
        self._set_status("New empty ZIP — use Add to put files in it.")

    def cmd_add(self):
        if not self.archive_path:
            self.cmd_new()
            if not self.archive_path:
                return
        if eng.detect_format(self.archive_path) not in ("zip", "unknown"):
            messagebox.showinfo(
                APP_TITLE,
                "UnRAR can create/update ZIP archives only.\n"
                "(RAR writing is proprietary — use Rar.exe for that.)")
            return
        files = list(filedialog.askopenfilenames(title="Files to add"))
        folder = filedialog.askdirectory(
            title="Or pick a folder to add (Cancel to skip)")
        if folder:
            files.append(folder)
        if not files:
            return
        level = self._level.get()
        self._run_bg(
            lambda: eng.add_to_zip(self.archive_path, files, level),
            f"Added {{result}} file(s) at level {level}")

    def cmd_extract(self):
        if not self.archive_path:
            messagebox.showinfo(APP_TITLE, "Open an archive first.")
            return
        dest = filedialog.askdirectory(title="Extract to folder")
        if not dest:
            return
        members = self._selected_names() or None
        pwd = None
        if self.info and eng.is_encrypted(self.info):
            pwd = simpledialog.askstring(APP_TITLE,
                                         "Password for encrypted entries:",
                                         show="*")
        self._run_bg(
            lambda: eng.extract_archive(self.archive_path, dest, members, pwd),
            "Extracted to " + dest)

    def cmd_test(self):
        if not self.archive_path:
            messagebox.showinfo(APP_TITLE, "Open an archive first.")
            return

        def work():
            ok, detail = eng.test_archive(self.archive_path)
            if not ok:
                raise eng.EngineError(detail)
            return detail

        self._run_bg(work, "Test: {result}")

    def cmd_view(self):
        names = self._selected_names()
        if not names or not self.archive_path:
            return
        name = names[0]
        tmp = tempfile.mkdtemp(prefix="unrar_view_")
        pwd = None
        if self.info and eng.is_encrypted(self.info):
            pwd = simpledialog.askstring(APP_TITLE, "Password:", show="*")

        def work():
            eng.extract_archive(self.archive_path, tmp, [name], pwd)
            return name

        self._run_bg(work, "Viewed {result}")
        # launch the file once extraction completes (max ~30s)
        attempts = [0]

        def launch():
            attempts[0] += 1
            target = os.path.join(tmp, name)
            if os.path.isfile(target):
                os.startfile(target)            # noqa: S606 — local file open
            elif attempts[0] < 60:
                self.after(500, launch)

        self.after(500, launch)

    def cmd_delete(self):
        names = self._selected_names()
        if not names or not self.archive_path:
            return
        if eng.detect_format(self.archive_path) != "zip":
            messagebox.showinfo(
                APP_TITLE, "Delete is supported for ZIP archives only.")
            return
        if not messagebox.askyesno(
                APP_TITLE, f"Delete {len(names)} entr(y/ies)?"):
            return

        def work():
            import zipfile
            src = self.archive_path
            drop = set(names)
            tmp = src + ".tmp"
            with zipfile.ZipFile(src) as zin, \
                    zipfile.ZipFile(tmp, "w", zipfile.ZIP_DEFLATED) as zout:
                zout.comment = zin.comment
                for item in zin.infolist():
                    if item.filename not in drop:
                        zout.writestr(item, zin.read(item.filename))
            os.replace(tmp, src)
            return len(drop)

        self._run_bg(work, "Deleted {result} entr(y/ies)")

    def cmd_comment(self):
        if not self.archive_path:
            messagebox.showinfo(APP_TITLE, "Open a ZIP archive first.")
            return
        if eng.detect_format(self.archive_path) != "zip":
            messagebox.showinfo(APP_TITLE, "Comments supported for ZIP only.")
            return
        import zipfile
        with zipfile.ZipFile(self.archive_path) as zf:
            cur = zf.comment.decode("utf-8", "replace")
        text = simpledialog.askstring(APP_TITLE, "Archive comment:",
                                      initialvalue=cur)
        if text is None:
            return

        def work():
            with zipfile.ZipFile(self.archive_path, "a") as zf:
                zf.comment = text.encode("utf-8")
            return len(text)

        self._run_bg(work, "Comment saved ({result} chars)")

    def cmd_lock(self):
        if not self.archive_path:
            messagebox.showinfo(APP_TITLE, "Open an archive first.")
            return
        st = os.stat(self.archive_path).st_mode
        try:
            if st & stat.S_IWRITE:
                os.chmod(self.archive_path, stat.S_IREAD)
                self._set_status("Archive locked (read-only).")
            else:
                os.chmod(self.archive_path, stat.S_IWRITE | stat.S_IREAD)
                self._set_status("Archive unlocked (writable).")
        except OSError as e:
            messagebox.showerror(APP_TITLE, str(e))

    def cmd_info(self):
        if not self.archive_path or not self.info:
            messagebox.showinfo(APP_TITLE, "Open an archive first.")
            return
        i = self.info
        stt = os.stat(self.archive_path)
        total = sum(e.size for e in i.entries)
        packed = sum(e.packed for e in i.entries)
        ro = not (stt.st_mode & stat.S_IWRITE)
        lines = [
            f"Archive: {i.path}",
            f"Format: {i.format.upper()}{' (locked)' if ro else ''}",
            f"Entries: {len(i.entries)}",
            f"Unpacked size: {fmt_size(total)}",
            f"Packed size: {fmt_size(packed)}",
            f"File size: {fmt_size(stt.st_size)}",
            f"Modified: {datetime.fromtimestamp(stt.st_mtime):%Y-%m-%d %H:%M}",
        ]
        if packed and total:
            lines.append(f"Overall ratio: {100 - packed * 100 // total}%")
        if i.comment:
            lines.append(f"Comment: {i.comment}")
        if i.needs_backend:
            lines.append(f"Note: {i.needs_backend} needs an external tool.")
        if eng.is_encrypted(i):
            lines.append("Contains encrypted entries.")
        messagebox.showinfo(f"{APP_TITLE} — Archive information",
                            "\n".join(lines))

    def cmd_about(self):
        messagebox.showinfo(
            f"About {APP_TITLE}",
            f"{APP_TITLE} {APP_VERSION}\n\n"
            "An open-source WinRAR-style archive manager (MIT license).\n"
            "Create/read/test ZIP natively; read/extract RAR via the\n"
            "freeware UnRAR backend; read TAR/GZ/BZ2/XZ.\n\n"
            "Format detection is content-signature based, like the original.")


def main(argv: list[str]) -> int:
    archive = argv[1] if len(argv) > 1 else None
    app = UnRARApp(archive)
    app.mainloop()
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
