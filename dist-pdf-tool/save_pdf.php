<?php
// Example: receive the edited PDF (multipart fields: file, name, save_as). Add your own authentication.
if ($_SERVER['REQUEST_METHOD'] !== 'POST' || !isset($_FILES['file']) || $_FILES['file']['error'] !== UPLOAD_ERR_OK) {
    http_response_code(400); exit('bad request');
}
$tmp = $_FILES['file']['tmp_name'];
if (file_get_contents($tmp, false, null, 0, 5) !== '%PDF-') { http_response_code(415); exit('not a pdf'); }
$rawName = $_POST['name'] ?? 'document.pdf';
$rawName = str_replace(['\\', '/'], '/', $rawName);
$base = substr(strrchr('/' . $rawName, '/'), 1);
$name = preg_replace('/[^\p{L}\p{M}\p{N}._-]/u', '_', $base);
if (!preg_match('/\.pdf$/i', $name)) { $name .= '.pdf'; }
if ($name === '.pdf' || $name === '') { $name = 'document.pdf'; }
$dir = __DIR__ . '/../pdf_store';
if (!is_dir($dir)) { mkdir($dir, 0750, true); }
if (!move_uploaded_file($tmp, "$dir/$name")) { http_response_code(500); exit('save failed'); }
echo 'ok';
