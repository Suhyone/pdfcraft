(() => {
  const canvas = document.getElementById("pdfcraft");
  if (!canvas) return;

  // Only same-origin addresses are accepted, so a crafted link cannot make the page fetch or post elsewhere.
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
  const loadUrl = sameOriginUrl(params.get("load_url") || params.get("pdf_url"), null); // e.g. get_pdf.php?id=123
  const saveUrl = sameOriginUrl(params.get("save_url"), "save_pdf.php");
  const docName = (params.get("name") || "document.pdf").trim() || "document.pdf";

  // 1. Load the PDF from PHP once PdfCraft is ready.
  canvas.addEventListener("pdfcraft:ready", async () => {
    if (!loadUrl) return;
    try {
      const res = await fetch(loadUrl, { credentials: "same-origin", cache: "no-store" });
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const bytes = await res.arrayBuffer();
      const head = new TextDecoder("ascii").decode(bytes.slice(0, 5));
      if (head !== "%PDF-") throw new Error("Invalid PDF response from server");
      canvas.dispatchEvent(new CustomEvent("pdfcraft:open", { detail: { name: docName, bytes } }));
    } catch (err) {
      console.error("bridge.js: failed to load PDF from PHP:", err);
      window.alert(`ไม่สามารถเปิดไฟล์ PDF จากเซิร์ฟเวอร์ได้ (${err.message})`);
    }
  });

  // 2. Save the PDF back to PHP (UTF-8 file names are fine).
  canvas.addEventListener("pdfcraft:save", async (event) => {
    const { name, bytes, saveAs } = event.detail;
    const finalName = (name || docName).trim() || "document.pdf";
    try {
      const form = new FormData();
      form.append("file", new Blob([bytes], { type: "application/pdf" }), finalName);
      form.append("name", finalName);
      form.append("save_as", saveAs ? "1" : "0");
      const res = await fetch(saveUrl, { method: "POST", body: form, credentials: "same-origin" });
      const text = (await res.text()).trim();
      if (!res.ok || (text && text !== "ok" && !text.startsWith("{"))) {
        throw new Error(text || `HTTP ${res.status}`);
      }
    } catch (err) {
      console.error("bridge.js: failed to save PDF to PHP:", err);
      window.alert(`บันทึกไฟล์ PDF ไปยังเซิร์ฟเวอร์ไม่สำเร็จ (${err.message})`);
    }
  });
})();
