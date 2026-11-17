<?php
// Example: serve a stored PDF. Open the tool as index.html?load_url=get_pdf.php%3Fid%3D123&name=file.pdf
// Replace the lookup with your own storage and access checks.
$id = preg_replace('/[^A-Za-z0-9_-]/', '', $_GET['id'] ?? '');
$path = __DIR__ . '/../pdf_store/' . $id . '.pdf';
if ($id === '' || !is_file($path)) { http_response_code(404); exit; }
header('Content-Type: application/pdf');
header('Content-Length: ' . filesize($path));
readfile($path);
