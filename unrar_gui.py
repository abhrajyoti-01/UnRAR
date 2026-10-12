"""UnRAR — WinRAR-style archive manager GUI (Tkinter, stdlib only)."""

from __future__ import annotations

import os
import queue
import subprocess
import sys
import tempfile
import threading
import tkinter as tk
from tkinter import filedialog, messagebox, simpledialog, ttk

import unrar_engine as eng

APP_TITLE = "UnRAR"
APP_VERSION = "1.0"


def fmt_size(n: int) -> str:
    for unit in ("B", "KB", "MB", "GB", "TB"):
        if n < 1024 or unit == "TB":
            return f"{n:,.0f} {unit}" if unit == "B" else f"{n:,.1f} {unit}"
        n /= 1024
    return str(n)


def fmt_time(dt) -> str:
    return dt.strftime("%Y-%m-%d %H:%M") if dt else ""


class UnRARApp(tk.Tk):
    def __init__(self, archive: str | None = None):
        super().__init__()
        self.title(APP_TITLE)
        self.geometry("980x560")
        self.minsize(640, 320)
        self._icon_text()

        self.archive_path: str | None = None
        self.info: eng.ArchiveInfo | None = None
        self._q: queue.Queue = queue.Queue()
        self._build_menu()
        self._build_toolbar()
        self._build_list()
        self._build_statusbar()
        self.after(80, self._poll)

        if archive:
            self.open_archive(archive)
        else:
            self._set_status("Ready. Open an archive (Ctrl+O) or create one (Ctrl+N).")

    # ---------- UI construction ----------

    def _icon_text(self):
        try:
            self.iconbitmap(default="")
        except tk.TclError:
            pass

    def _build_menu(self):
        menubar = tk.Menu(self)
        m_file = tk.Menu(menubar, tearoff=0)
        m_file.add_command(label="&Open archive…\tCtrl+O", command=self.cmd_open)
        m_file.add_command(label="&New archive…\tCtrl+N", command=self.cmd_new)
        m_file.add_separator()
        m_file.add_command(label="&Extract to…\tCtrl+E", command=self.cmd_extract)
        m_file.add_command(label="E&xit", command=self.destroy)
        menubar.add_cascade(label="&File", menu=m_file)

        m_cmd = tk.Menu(menubar, tearoff=0)
        m_cmd.add_command(label="&Add files to archive…\tCtrl+A",
                          command=self.cmd_add)
        m_cmd.add_command(label="&Test archive\tCtrl+T", command=self.cmd_test)
        m_cmd.add_command(label="&View selected file\tEnter",
                          command=self.cmd_view)
        m_cmd.add_command(label="Archive &information\tCtrl+I",
                          command=self.cmd_info)
        menubar.add_cascade(label="&Commands", menu=m_cmd)

        m_help = tk.Menu(menubar, tearoff=0)
        m_help.add_command(label="&About UnRAR", command=self.cmd_about)
        menubar.add_cascade(label="&Help", menu=m_help)
        self.config(menu=menubar)

        self.bind("<Control-o>", lambda e: self.cmd_open())
        self.bind("<Control-n>", lambda e: self.cmd_new())
        self.bind("<Control-e>", lambda e: self.cmd_extract())
        self.bind("<Control-a>", lambda e: self.cmd_add())
        self.bind("<Control-t>", lambda e: self.cmd_test())
        self.bind("<Control-i>", lambda e: self.cmd_info())
        self.bind("<Return>", lambda e: self.cmd_view())

    def _build_toolbar(self):
        bar = ttk.Frame(self, padding=(4, 4))
        bar.pack(side=tk.TOP, fill=tk.X)
        for text, cmd in (
            ("Add", self.cmd_add), ("Extract to", self.cmd_extract),
            ("Test", self.cmd_test), ("View", self.cmd_view),
            ("Delete", self.cmd_delete), ("Info", self.cmd_info),
            ("Open", self.cmd_open),
        ):
            ttk.Button(bar, text=text, command=cmd, width=11).pack(
                side=tk.LEFT, padx=2)

    def _build_list(self):
        frame = ttk.Frame(self)
        frame.pack(fill=tk.BOTH, expand=True)
        cols = ("name", "size", "packed", "ratio", "modified", "crc", "method")
        self.tree = ttk.Treeview(frame, columns=cols, show="headings",
                                 selectmode="extended")
        heads = {
            "name": ("Name", 300), "size": ("Size", 90),
            "packed": ("Packed", 90), "ratio": ("Ratio", 60),
            "modified": ("Modified", 130), "crc": ("CRC32", 90),
            "method": ("Method", 90),
        }
        for c, (label, w) in heads.items():
            self.tree.heading(c, text=label,
                              command=lambda c=c: self._sort_by(c))
            anchor = tk.W if c in ("name", "method") else tk.E
            self.tree.column(c, width=w, anchor=anchor)
        vsb = ttk.Scrollbar(frame, orient=tk.VERTICAL, command=self.tree.yview)
        self.tree.configure(yscrollcommand=vsb.set)
        self.tree.pack(side=tk.LEFT, fill=tk.BOTH, expand=True)
        vsb.pack(side=tk.RIGHT, fill=tk.Y)
        self.tree.bind("<Double-1>", lambda e: self.cmd_view())
        self.tree.bind("<Delete>", lambda e: self.cmd_delete())

    def _build_statusbar(self):
        self.status = ttk.Label(self, text="", anchor=tk.W, relief=tk.SUNKEN,
                                padding=(6, 2))
        self.status.pack(side=tk.BOTTOM, fill=tk.X)

    # ---------- helpers ----------

    def _set_status(self, text: str):
        self.status.config(text=text)

    def _sort_by(self, col):
        rows = [(self.tree.set(i, col), i) for i in self.tree.get_children("")]
        try:
            rows.sort(key=lambda r: float(r[0].replace(",", "").replace("%", "")
                                          .split()[0] or 0))
        except (ValueError, IndexError):
            rows.sort()
        for pos, (_, iid) in enumerate(rows):
            self.tree.move(iid, "", pos)

    def _selected_names(self) -> list[str]:
        return [self.tree.item(i, "values")[0] for i in self.tree.selection()]

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

    def _poll(self):
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
        self.after(80, self._poll)

    def _reload(self):
        if self.archive_path:
            self.open_archive(self.archive_path, silent=True)

    def _populate(self):
        self.tree.delete(*self.tree.get_children())
        for e in self.info.entries:
            crc = f"{e.crc:08X}" if e.crc is not None else ""
            self.tree.insert("", tk.END, values=(
                e.name, "" if e.is_dir else fmt_size(e.size),
                "" if e.is_dir else fmt_size(e.packed), e.ratio,
                fmt_time(e.mtime), crc, e.method))

    # ---------- commands ----------

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
        self.title(f"{APP_TITLE} — {path}")
        self._populate()
        n = len(info.entries)
        note = ""
        if info.needs_backend:
            note = f"  [{info.needs_backend} contents need an external tool]"
        if eng.is_encrypted(info):
            note += "  [encrypted]"
        self._set_status(f"{info.format.upper()} archive — {n} entries.{note}")

    def cmd_new(self):
        path = filedialog.asksaveasfilename(
            title="New archive", defaultextension=".zip",
            filetypes=[("ZIP archive", "*.zip")])
        if not path:
            return
        self.archive_path = path
        self.info = eng.ArchiveInfo(path=path, format="zip")
        self.title(f"{APP_TITLE} — {path}")
        self._populate()
        self._set_status("New empty ZIP archive. Use Add to put files in it.")

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
        folder = filedialog.askdirectory(title="Or pick a folder to add (Cancel to skip)")
        if folder:
            files.append(folder)
        if not files:
            return
        level = simpledialog.askstring(
            APP_TITLE, "Compression (Store/Fastest/Fast/Normal/Good/Best):",
            initialvalue="Normal") or "Normal"
        self._run_bg(
            lambda: eng.add_to_zip(self.archive_path, files, level),
            "Added {{result}} file(s) — level {}".format(level))

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
            pwd = simpledialog.askstring(
                APP_TITLE, "Password for encrypted entries:",
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
        def work():
            out = eng.extract_archive(self.archive_path, tmp, [name])[0]
            return out
        def after():
            target = os.path.join(tmp, name)
            if os.path.isfile(target):
                os.startfile(target)            # noqa: S606 — local file open
        self._run_bg(lambda: (work(), after())[0],
                     "Viewing {result}")

    def cmd_delete(self):
        names = self._selected_names()
        if not names or not self.archive_path:
            return
        if eng.detect_format(self.archive_path) != "zip":
            messagebox.showinfo(APP_TITLE,
                                "Delete is supported for ZIP archives only.")
            return
        if not messagebox.askyesno(
                APP_TITLE, f"Delete {len(names)} entr(y/ies) from archive?"):
            return
        def work():
            import zipfile
            src = self.archive_path
            drop = set(names)
            tmp = src + ".tmp"
            with zipfile.ZipFile(src) as zin, \
                 zipfile.ZipFile(tmp, "w", zipfile.ZIP_DEFLATED) as zout:
                for item in zin.infolist():
                    if item.filename not in drop:
                        zout.writestr(item, zin.read(item.filename))
            os.replace(tmp, src)
            return len(drop)
        self._run_bg(work, "Deleted {result} entr(y/ies)")

    def cmd_info(self):
        if not self.archive_path or not self.info:
            messagebox.showinfo(APP_TITLE, "Open an archive first.")
            return
        i = self.info
        st = os.stat(self.archive_path)
        total = sum(e.size for e in i.entries)
        packed = sum(e.packed for e in i.entries)
        lines = [
            f"Archive: {i.path}",
            f"Format: {i.format.upper()}",
            f"Entries: {len(i.entries)}",
            f"Unpacked size: {fmt_size(total)}",
            f"Packed size: {fmt_size(packed)}",
            f"File size on disk: {fmt_size(st.st_size)}",
        ]
        if packed and total:
            lines.append(f"Overall ratio: {100 - packed * 100 // total}%")
        if i.comment:
            lines.append(f"Comment: {i.comment}")
        if i.needs_backend:
            lines.append(
                f"Note: {i.needs_backend} content needs an external tool.")
        if eng.is_encrypted(i):
            lines.append("Contains encrypted entries.")
        messagebox.showinfo(f"{APP_TITLE} — Archive information",
                            "\n".join(lines))

    def cmd_about(self):
        messagebox.showinfo(
            f"About {APP_TITLE}",
            f"{APP_TITLE} {APP_VERSION}\n\n"
            "A WinRAR-style archive manager.\n"
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
