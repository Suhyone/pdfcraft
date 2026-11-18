<?php
// ตัวอย่างหน้าเว็บระบบเอกสารของบริษัท ที่เรียกใช้งาน PdfCraft เมื่อกดปุ่มเปิดไฟล์ PDF
$currentUser = "สมชาย ใจดี (ฝ่ายตรวจสอบ)";
$documents = [
    ["id" => "contract_001", "name" => "สัญญาจ้างบริการ_2026.pdf", "updated" => "2026-11-17 08:30"],
    ["id" => "quote_002",    "name" => "ใบเสนอราคา_Q4.pdf",        "updated" => "2026-11-17 09:00"],
];
?>
<!doctype html>
<html lang="th">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>ระบบจัดการเอกสารภายในบริษัท — ตัวอย่างการเชื่อมต่อ PdfCraft</title>
  <style>
    body { font-family: system-ui, -apple-system, "Segoe UI", Tahoma, sans-serif; background: #f8fafc; color: #0f172a; margin: 0; padding: 32px; }
    .container { max-width: 900px; margin: 0 auto; background: #fff; border-radius: 12px; padding: 24px; box-shadow: 0 4px 16px rgba(0,0,0,0.06); }
    h1 { margin-top: 0; font-size: 20px; }
    table { width: 100%; border-collapse: collapse; margin-top: 16px; }
    th, td { text-align: left; padding: 12px; border-bottom: 1px solid #e2e8f0; font-size: 14px; }
    th { background: #f1f5f9; font-weight: 600; }
    .btn { display: inline-flex; align-items: center; gap: 6px; padding: 7px 14px; border-radius: 6px; font-size: 13px; font-weight: 600; cursor: pointer; text-decoration: none; border: none; }
    .btn-primary { background: #2563eb; color: #fff; }
    .btn-primary:hover { background: #1d4ed8; }
    .btn-outline { background: #fff; color: #334155; border: 1px solid #cbd5e1; }
    .status-badge { display: inline-block; padding: 3px 8px; border-radius: 999px; font-size: 12px; background: #dcfce7; color: #166534; }

    /* Fullscreen PDF Editor Modal */
    .editor-overlay { position: fixed; inset: 0; background: rgba(15,23,42,0.8); z-index: 9999; display: none; flex-direction: column; }
    .editor-overlay.open { display: flex; }
    .editor-topbar { height: 48px; background: #0f172a; color: #f8fafc; display: flex; align-items: center; justify-content: space-between; padding: 0 16px; font-size: 14px; }
    .editor-topbar .doc-title { font-weight: 600; display: flex; align-items: center; gap: 10px; }
    .editor-topbar .dirty-dot { width: 8px; height: 8px; border-radius: 50%; background: #f59e0b; display: none; }
    .editor-topbar .dirty-dot.show { display: inline-block; }
    .editor-frame { flex: 1; width: 100%; border: 0; background: #19191c; }
    @media (max-width: 640px) {
      body { padding: 8px; }
      .container { padding: 14px; border-radius: 8px; }
      table, thead, tbody, tr, th, td { display: block; }
      thead { display: none; }
      tr { border: 1px solid #e2e8f0; border-radius: 8px; margin-bottom: 10px; padding: 6px 0; }
      td { border-bottom: 0; padding: 6px 12px; overflow-wrap: anywhere; }
      td .btn { margin: 2px 4px 2px 0; }
      .editor-topbar { padding: 0 8px; font-size: 13px; gap: 8px; }
      .editor-topbar .doc-title { min-width: 0; overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
      .btn-close { padding: 6px 10px; white-space: nowrap; }
    }
    .btn-close { background: #ef4444; color: #fff; border: none; padding: 6px 14px; border-radius: 6px; font-weight: 600; cursor: pointer; }
  </style>
</head>
<body>
  <div class="container">
    <h1>รายการเอกสาร PDF ของบริษัท (ผู้ใช้งาน: <?= htmlspecialchars($currentUser) ?>)</h1>
    <p style="color:#475569;font-size:14px;">เมื่อกดปุ่ม <b>"เปิด / แก้ไข PDF"</b> ระบบจะเปิดโปรแกรมจัดการ PDF บนหน้าเว็บทันที และบันทึกกลับเข้าเซิร์ฟเวอร์เมื่อกด <code>Ctrl+S</code></p>

    <table>
      <thead>
        <tr>
          <th>รหัสเอกสาร</th>
          <th>ชื่อไฟล์</th>
          <th>แก้ไขล่าสุด</th>
          <th>สถานะ</th>
          <th>การจัดการ</th>
        </tr>
      </thead>
      <tbody>
        <?php foreach ($documents as $doc):
          $editorUrl = "index.html?load_url=" . rawurlencode("get_pdf.php?id=" . $doc['id'])
                     . "&save_url=" . rawurlencode("save_pdf.php?id=" . $doc['id'])
                     . "&name=" . rawurlencode($doc['name'])
                     . "&author=" . rawurlencode($currentUser);
        ?>
        <tr>
          <td><code><?= htmlspecialchars($doc['id']) ?></code></td>
          <td><b><?= htmlspecialchars($doc['name']) ?></b></td>
          <td id="time-<?= htmlspecialchars($doc['id']) ?>"><?= htmlspecialchars($doc['updated']) ?></td>
          <td><span class="status-badge" id="status-<?= htmlspecialchars($doc['id']) ?>">พร้อมใช้งาน</span></td>
          <td>
            <button class="btn btn-primary js-open-pdf" type="button"
                    data-id="<?= htmlspecialchars($doc['id'], ENT_QUOTES) ?>"
                    data-name="<?= htmlspecialchars($doc['name'], ENT_QUOTES) ?>"
                    data-url="<?= htmlspecialchars($editorUrl, ENT_QUOTES) ?>">
              เปิด / แก้ไข PDF (Modal)
            </button>
            <a class="btn btn-outline" href="<?= htmlspecialchars($editorUrl) ?>" target="_blank" rel="noopener">
              เปิดแท็บใหม่
            </a>
          </td>
        </tr>
        <?php endforeach; ?>
      </tbody>
    </table>
  </div>

  <!-- Modal แสดงโปรแกรมแก้ไข PDF เต็มจอ -->
  <div class="editor-overlay" id="pdf-modal">
    <div class="editor-topbar">
      <div class="doc-title">
        <span class="dirty-dot" id="dirty-indicator" title="มีการแก้ไขที่ยังไม่ได้บันทึก"></span>
        <span id="modal-doc-name">กำลังเปิดเอกสาร...</span>
      </div>
      <button class="btn-close" id="close-modal" type="button">ปิดหน้าต่าง (X)</button>
    </div>
    <iframe class="editor-frame" id="pdf-iframe" allow="fullscreen; clipboard-read; clipboard-write"></iframe>
  </div>

  <script>
    let activeDocId = null;
    let isDirty = false;

    function openPdfModal(docId, docName, url) {
      activeDocId = docId;
      isDirty = false;
      document.getElementById("dirty-indicator").classList.remove("show");
      document.getElementById("modal-doc-name").textContent = docName;
      const frame = document.getElementById("pdf-iframe");
      frame.onload = () => { try { frame.contentWindow.focus(); } catch {} };
      frame.src = url;
      document.getElementById("pdf-modal").classList.add("open");
    }

    // Names and URLs reach the script through data-* attributes, never inside JavaScript text.
    document.querySelectorAll(".js-open-pdf").forEach((btn) => {
      btn.addEventListener("click", () => openPdfModal(btn.dataset.id, btn.dataset.name, btn.dataset.url));
    });
    document.getElementById("close-modal").addEventListener("click", () => closePdfModal());

    function closePdfModal() {
      if (isDirty && !window.confirm("เอกสารมีการแก้ไขที่ยังไม่ได้บันทึก ต้องการปิดใช่หรือไม่?")) {
        return;
      }
      document.getElementById("pdf-iframe").src = "about:blank";
      document.getElementById("pdf-modal").classList.remove("open");
      activeDocId = null;
      isDirty = false;
    }

    // Ctrl+S / Cmd+S on the host page while the editor is open: save through the editor, not the browser.
    window.addEventListener("keydown", (e) => {
      if (!document.querySelector("#pdf-modal.open")) return;
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "s") {
        e.preventDefault();
        const frame = document.getElementById("pdf-iframe");
        frame.contentWindow?.postMessage({ type: "company-pdf:trigger-save" }, window.location.origin);
        try { frame.contentWindow?.focus(); } catch {}
      }
    });

    // รับ Event จาก bridge.js เมื่อผู้ใช้แก้ไขหรือกด Save ในโปรแกรม PDF
    window.addEventListener("message", (event) => {
      if (event.origin !== window.location.origin || !event.data) return;
      const { type, name, dirty } = event.data;

      if (type === "company-pdf:dirty") {
        isDirty = !!dirty;
        document.getElementById("dirty-indicator").classList.toggle("show", isDirty);
      } else if (type === "company-pdf:saved" && activeDocId) {
        isDirty = false;
        document.getElementById("dirty-indicator").classList.remove("show");
        const statusEl = document.getElementById("status-" + activeDocId);
        const timeEl = document.getElementById("time-" + activeDocId);
        if (statusEl) statusEl.textContent = "บันทึกแล้ว (" + name + ")";
        if (timeEl) timeEl.textContent = new Date().toLocaleString("th-TH");
      }
    });
  </script>
</body>
</html>
