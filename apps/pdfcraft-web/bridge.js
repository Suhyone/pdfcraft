(() => {
  const canvas = document.getElementById("pdfcraft");
  if (!canvas) return;

  const params = new URLSearchParams(window.location.search);
  const loadUrl = params.get("load_url") || params.get("pdf_url"); // e.g. get_pdf.php?id=123
  const saveUrl = params.get("save_url") || "save_pdf.php";
  const docName = params.get("name") || "document.pdf";

  // 1. When PdfCraft WASM is ready, fetch the PDF from PHP and hand it over via pdfcraft:open.
  canvas.addEventListener("pdfcraft:ready", async () => {
    if (!loadUrl) return;
    try {
      const res = await fetch(loadUrl, { credentials: "same-origin" });
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const bytes = await res.arrayBuffer();
      canvas.dispatchEvent(new CustomEvent("pdfcraft:open", { detail: { name: docName, bytes } }));
    } catch (err) {
      console.error("bridge.js: failed to load PDF from PHP:", err);
    }
  });

  // 2. When the user triggers Save / Save As, POST the bytes back to PHP.
  canvas.addEventListener("pdfcraft:save", async (event) => {
    const { name, bytes, saveAs } = event.detail;
    try {
      const form = new FormData();
      form.append("file", new Blob([bytes], { type: "application/pdf" }), name || docName);
      form.append("name", name || docName);
      form.append("save_as", saveAs ? "1" : "0");
      const res = await fetch(saveUrl, { method: "POST", body: form, credentials: "same-origin" });
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
    } catch (err) {
      console.error("bridge.js: failed to save PDF to PHP:", err);
    }
  });
})();
