(() => {
  const canvas = document.getElementById("pdfcraft");
  if (!canvas) return;

  // 1. Security: allow only same-origin URLs
  const sameOriginUrl = (raw, fallback) => {
    if (!raw) return fallback;
    try {
      const u = new URL(raw, window.location.href);
      return u.origin === window.location.origin ? u.href : fallback;
    } catch {
      return fallback;
    }
  };

  const params = new URLSearchParams(window.location.search);
  const loadUrl = sameOriginUrl(params.get("load_url") || params.get("pdf_url"), null);
  const saveUrl = sameOriginUrl(params.get("save_url"), "save_pdf.php");
  const docName = (params.get("name") || "document.pdf").trim() || "document.pdf";
  const hideHelp = params.get("help") === "0";

  // 2. Inject lightweight UI styles (Toast + First-Time Onboarding Modal + Floating Help Button)
  const style = document.createElement("style");
  style.textContent = `
    .pc-toast {
      position: fixed; top: 14px; left: 50%; transform: translateX(-50%);
      background: #1e293b; color: #f8fafc; padding: 8px 16px; border-radius: 8px;
      font: 500 13.5px/1.4 system-ui, -apple-system, "Segoe UI", sans-serif;
      box-shadow: 0 8px 24px rgba(0,0,0,0.28); z-index: 99998;
      display: none; align-items: center; gap: 8px; pointer-events: none;
      border: 1px solid rgba(255,255,255,0.12);
    }
    .pc-toast.show { display: inline-flex; }
    .pc-toast.ok { background: #065f46; }
    .pc-toast.err { background: #991b1b; }

    .pc-help-btn {
      position: fixed; right: 16px; bottom: 16px; z-index: 99990;
      background: #2563eb; color: #fff; border: none; border-radius: 999px;
      padding: 7px 14px; font: 600 13px/1 system-ui, -apple-system, "Segoe UI", sans-serif;
      cursor: pointer; box-shadow: 0 4px 14px rgba(37,99,235,0.4);
      display: inline-flex; align-items: center; gap: 6px; transition: background 0.15s;
    }
    .pc-help-btn:hover { background: #1d4ed8; }
    @media (max-width: 640px) {
      .pc-modal-backdrop { padding: 8px; }
      .pc-modal { width: 100%; max-width: 100%; max-height: calc(100vh - 16px); border-radius: 12px; }
      .pc-modal-header { padding: 14px 16px; }
      .pc-modal-header h2 { font-size: 16px; }
      .pc-modal-body { padding: 12px 16px; max-height: 56vh; }
      .pc-modal-footer { padding: 12px 16px; display: flex; flex-wrap: wrap; gap: 8px; }
      .pc-modal-footer button { flex: 1 1 100%; }
      .pc-help-btn { right: 8px; bottom: 8px; padding: 5px 10px; font-size: 12px; opacity: 0.85; }
    }

    .pc-modal-backdrop {
      position: fixed; inset: 0; background: rgba(15,23,42,0.65);
      backdrop-filter: blur(2px); z-index: 99999;
      display: none; align-items: center; justify-content: center; padding: 16px;
      font-family: system-ui, -apple-system, "Segoe UI", Tahoma, sans-serif;
    }
    .pc-modal-backdrop.open { display: flex; }
    .pc-modal {
      background: #ffffff; color: #0f172a; width: 100%; max-width: 580px;
      border-radius: 14px; box-shadow: 0 20px 50px rgba(0,0,0,0.35);
      overflow: hidden; animation: pcFadeIn 0.18s ease-out;
    }
    @keyframes pcFadeIn { from { opacity: 0; transform: translateY(8px); } to { opacity: 1; transform: translateY(0); } }
    .pc-modal-header {
      background: linear-gradient(135deg, #1e40af, #2563eb); color: #fff;
      padding: 18px 22px; display: flex; justify-content: space-between; align-items: center;
    }
    .pc-modal-header h2 { margin: 0; font-size: 18px; font-weight: 700; }
    .pc-modal-header p { margin: 4px 0 0; font-size: 13px; opacity: 0.9; }
    .pc-modal-body { padding: 18px 22px; max-height: 68vh; overflow-y: auto; }
    .pc-tool-list { display: grid; gap: 12px; margin: 0; padding: 0; list-style: none; }
    .pc-tool-item {
      display: flex; gap: 12px; align-items: flex-start;
      background: #f8fafc; border: 1px solid #e2e8f0; border-radius: 10px; padding: 11px 14px;
    }
    .pc-tool-badge {
      flex-shrink: 0; width: 32px; height: 32px; border-radius: 8px;
      background: #dbeafe; color: #1d4ed8; font-weight: 700; font-size: 14px;
      display: flex; align-items: center; justify-content: center;
    }
    .pc-tool-title { font-size: 14px; font-weight: 600; color: #0f172a; margin-bottom: 2px; }
    .pc-tool-desc { font-size: 13px; color: #475569; line-height: 1.45; }
    .pc-kbd {
      display: inline-block; padding: 1px 6px; font: 600 11.5px monospace;
      background: #e2e8f0; color: #1e293b; border-radius: 4px; border-bottom: 2px solid #cbd5e1;
    }
    .pc-modal-footer {
      padding: 14px 22px; background: #f1f5f9; border-top: 1px solid #e2e8f0;
      display: flex; justify-content: space-between; align-items: center; gap: 12px;
    }
    .pc-modal-footer label { font-size: 13px; color: #475569; display: flex; align-items: center; gap: 6px; cursor: pointer; }
    .pc-btn-primary {
      background: #2563eb; color: #fff; border: none; border-radius: 8px;
      padding: 9px 20px; font-size: 13.5px; font-weight: 600; cursor: pointer;
    }
    .pc-btn-primary:hover { background: #1d4ed8; }
  `;
  document.head.appendChild(style);

  // Toast helper
  const toast = document.createElement("div");
  toast.className = "pc-toast";
  document.body.appendChild(toast);
  let toastTimer = null;
  const showToast = (msg, kind = "", ms = 3200) => {
    clearTimeout(toastTimer);
    toast.textContent = msg;
    toast.className = `pc-toast show ${kind}`.trim();
    if (ms > 0) {
      toastTimer = setTimeout(() => toast.classList.remove("show"), ms);
    }
  };

  // 3. First-Time User Guide (5 Core Tools Only)
  const STORAGE_KEY = "pdfcraft_onboarding_v1";
  const backdrop = document.createElement("div");
  backdrop.className = "pc-modal-backdrop";
  backdrop.innerHTML = `
    <div class="pc-modal" role="dialog" aria-modal="true" aria-labelledby="pc-guide-title">
      <div class="pc-modal-header">
        <div>
          <h2 id="pc-guide-title">แนะนำการใช้งานระบบจัดการเอกสาร PDF</h2>
          <p>สรุป 5 เครื่องมือหลักที่ใช้บ่อยสำหรับการตรวจทานและแก้ไขเอกสาร</p>
        </div>
      </div>
      <div class="pc-modal-body">
        <ul class="pc-tool-list">
          <li class="pc-tool-item">
            <div class="pc-tool-badge">1</div>
            <div>
              <div class="pc-tool-title">การเลื่อนดู ซูม และค้นหาข้อความ</div>
              <div class="pc-tool-desc">หมุนลูกกลิ้งเมาส์เพื่อเลื่อนหน้า, กด <span class="pc-kbd">Ctrl</span> + ลูกกลิ้งเพื่อซูมเข้า-ออก และกด <span class="pc-kbd">Ctrl</span> + <span class="pc-kbd">F</span> เพื่อค้นหาคำในเอกสาร</div>
            </div>
          </li>
          <li class="pc-tool-item">
            <div class="pc-tool-badge">2</div>
            <div>
              <div class="pc-tool-title">การไฮไลต์ และใส่คอมเมนต์ (Add comments)</div>
              <div class="pc-tool-desc">เลือกเมนู <b>Add comments</b> แถบซ้ายมือ เพื่อป้ายสีไฮไลต์ข้อความสำคัญ ขีดเส้นใต้ หรือปักหมุดเขียนความเห็นลงบนจุดที่ต้องการ</div>
            </div>
          </li>
          <li class="pc-tool-item">
            <div class="pc-tool-badge">3</div>
            <div>
              <div class="pc-tool-title">การพิมพ์ข้อความเพิ่ม และลงลายเซ็น (Edit a PDF / Fill &amp; Sign)</div>
              <div class="pc-tool-desc">เลือกเมนู <b>Edit a PDF</b> หรือ <b>Fill &amp; Sign</b> เพื่อพิมพ์ข้อความลงในแบบฟอร์ม วางเครื่องหมายถูก หรือวางรูปลายเซ็น/ตราประทับ</div>
            </div>
          </li>
          <li class="pc-tool-item">
            <div class="pc-tool-badge">4</div>
            <div>
              <div class="pc-tool-title">การหมุนหน้า สลับหน้า หรือรวมไฟล์ (Organize pages)</div>
              <div class="pc-tool-desc">เลือกเมนู <b>Organize pages</b> เพื่อหมุนหน้ากระดาษที่กลับหัว ลบหน้าที่ไม่ต้องการ หรือแทรกหน้าจากไฟล์ PDF อื่น ลากไฟล์ PDF อื่นจากเครื่องมาวางบนหน้าจอเพื่อแทรกหรือรวมหน้าได้ทันที</div>
            </div>
          </li>
          <li class="pc-tool-item">
            <div class="pc-tool-badge">5</div>
            <div>
              <div class="pc-tool-title">การบันทึกไฟล์กลับเข้าระบบบริษัททันที (Save)</div>
              <div class="pc-tool-desc">เมื่อแก้ไขเสร็จ กด <span class="pc-kbd">Ctrl</span> + <span class="pc-kbd">S</span> (หรือเมนู <b>File ▸ Save</b>) ระบบจะบันทึกไฟล์กลับเข้าเซิร์ฟเวอร์โดยอัตโนมัติ ไม่ต้องดาวน์โหลดลงเครื่อง</div>
            </div>
          </li>
        </ul>
      </div>
      <div class="pc-modal-footer">
        <label>
          <input type="checkbox" id="pc-dont-show-again" checked />
          ไม่ต้องแสดงหน้าต่างนี้อีกในครั้งถัดไป
        </label>
        <button type="button" class="pc-btn-primary" id="pc-close-guide">เริ่มใช้งานเลย</button>
      </div>
    </div>
  `;
  document.body.appendChild(backdrop);

  // Stop keyboard events inside the modal from reaching the canvas
  backdrop.addEventListener("keydown", (e) => e.stopPropagation());

  const closeGuide = () => {
    const dontShow = document.getElementById("pc-dont-show-again")?.checked;
    try {
      if (dontShow) localStorage.setItem(STORAGE_KEY, "1");
      else localStorage.removeItem(STORAGE_KEY);
    } catch {}
    backdrop.classList.remove("open");
    canvas.focus();
  };

  document.getElementById("pc-close-guide")?.addEventListener("click", closeGuide);
  backdrop.addEventListener("click", (e) => {
    if (e.target === backdrop) closeGuide();
  });

  if (!hideHelp) {
    const helpBtn = document.createElement("button");
    helpBtn.type = "button";
    helpBtn.className = "pc-help-btn";
    helpBtn.textContent = "? คู่มือใช้งาน";
    helpBtn.title = "เปิดคู่มือแนะนำเครื่องมือหลัก";
    helpBtn.addEventListener("click", () => backdrop.classList.add("open"));
    document.body.appendChild(helpBtn);

    try {
      if (localStorage.getItem(STORAGE_KEY) !== "1") {
        backdrop.classList.add("open");
      }
    } catch {
      backdrop.classList.add("open");
    }
  }

  // 4. Load the PDF from PHP once PdfCraft WASM is ready
  if (loadUrl) {
    showToast(`กำลังเปิดเอกสาร "${docName}"...`, "", 0);
  }

  canvas.addEventListener("pdfcraft:ready", async () => {
    if (window.parent && window.parent !== window) {
      window.parent.postMessage({ type: "company-pdf:ready", name: docName }, window.location.origin);
    }
    if (!loadUrl) return;
    try {
      const res = await fetch(loadUrl, { credentials: "same-origin", cache: "no-store" });
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const bytes = await res.arrayBuffer();
      const head = new TextDecoder("ascii").decode(bytes.slice(0, 5));
      if (head !== "%PDF-") throw new Error("Invalid PDF response from server");
      canvas.dispatchEvent(new CustomEvent("pdfcraft:open", { detail: { name: docName, bytes } }));
      showToast(`เปิดเอกสาร "${docName}" เรียบร้อยแล้ว`, "ok", 2200);
    } catch (err) {
      console.error("bridge.js: failed to load PDF from PHP:", err);
      showToast(`ไม่สามารถเปิดไฟล์ PDF ได้ (${err.message})`, "err", 6000);
      window.alert(`ไม่สามารถเปิดไฟล์ PDF จากเซิร์ฟเวอร์ได้ (${err.message})`);
    }
  });

  // Ask from the host page: save through the app, same origin only.
  window.addEventListener("message", (event) => {
    if (event.origin !== window.location.origin || event.data?.type !== "company-pdf:trigger-save") return;
    canvas.focus();
    if (typeof window.pdfcraft?.execute === "function") {
      try { window.pdfcraft.execute("file.save"); return; } catch (err) { console.warn("bridge.js: execute failed", err); }
    }
    const init = { key: "s", code: "KeyS", ctrlKey: true, bubbles: true, cancelable: true };
    canvas.dispatchEvent(new KeyboardEvent("keydown", init));
  });

  // 5. Forward dirty state to parent company page (if embedded in iframe)
  canvas.addEventListener("pdfcraft:dirty", (event) => {
    if (window.parent && window.parent !== window) {
      window.parent.postMessage(
        { type: "company-pdf:dirty", name: docName, dirty: !!event.detail?.dirty },
        window.location.origin
      );
    }
  });

  // 6. Save the PDF back to PHP (UTF-8 filenames supported)
  canvas.addEventListener("pdfcraft:save", async (event) => {
    const { name, bytes, saveAs } = event.detail;
    const finalName = (name || docName).trim() || "document.pdf";
    showToast(`กำลังบันทึก "${finalName}" เข้าสู่ระบบ...`, "", 0);
    try {
      const form = new FormData();
      form.append("file", new Blob([bytes], { type: "application/pdf" }), finalName);
      form.append("name", finalName);
      form.append("save_as", saveAs ? "1" : "0");
      const res = await fetch(saveUrl, { method: "POST", body: form, credentials: "same-origin" });
      const text = (await res.text()).trim();
      if (!res.ok || (text && text !== "ok" && !text.startsWith("{"))) {
        // An error page is not shown whole: give the status, or a short message from the server.
        throw new Error(!res.ok || !text ? `HTTP ${res.status}` : text.slice(0, 120));
      }
      showToast(`บันทึก "${finalName}" เรียบร้อยแล้ว`, "ok", 3500);
      if (window.parent && window.parent !== window) {
        window.parent.postMessage(
          { type: "company-pdf:saved", name: finalName, saveAs: !!saveAs },
          window.location.origin
        );
      }
    } catch (err) {
      console.error("bridge.js: failed to save PDF to PHP:", err);
      showToast(`บันทึกไม่สำเร็จ (${err.message})`, "err", 6000);
      window.alert(`บันทึกไฟล์ PDF ไปยังเซิร์ฟเวอร์ไม่สำเร็จ (${err.message})`);
    }
  });
})();
