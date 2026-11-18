<?php
// Example: serve a stored PDF. Open the tool as index.html?load_url=get_pdf.php%3Fid%3D123&name=file.pdf
// Replace the lookup with your own storage and access checks.
$id = $_GET['id'] ?? '';
if (!is_string($id) || !preg_match('/^[A-Za-z0-9_-]{1,64}$/', $id)) {
    http_response_code(400); exit('invalid id');
}
$path = __DIR__ . '/../pdf_store/' . $id . '.pdf';
if (!is_file($path)) { http_response_code(404); exit; }
header('Content-Type: application/pdf');
header('Content-Length: ' . filesize($path));
readfile($path);
