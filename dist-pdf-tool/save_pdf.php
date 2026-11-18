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
// Drop one trailing .pdf, turn every remaining dot into _ (blocks shell.php.pdf), add a single .pdf.
$stem = preg_replace('/\.pdf$/i', '', $name);
$stem = str_replace('.', '_', $stem);
if ($stem === '') { $stem = 'document'; }
$name = $stem . '.pdf';
// With ?id=<id> the file is stored as <id>.pdf, which is the name get_pdf.php reads.
if (isset($_GET['id'])) {
    $id = $_GET['id'];
    if (!is_string($id) || !preg_match('/^[A-Za-z0-9_-]{1,64}$/', $id)) {
        http_response_code(400); exit('invalid id');
    }
    $name = $id . '.pdf';
}
$dir = __DIR__ . '/../pdf_store';
if (!is_dir($dir)) { mkdir($dir, 0750, true); }
$ht = "$dir/.htaccess";
if (!is_file($ht)) {
    file_put_contents($ht, "<IfModule mod_authz_core.c>
  Require all denied
</IfModule>
<IfModule !mod_authz_core.c>
  Order deny,allow
  Deny from all
</IfModule>
");
}
if (!move_uploaded_file($tmp, "$dir/$name")) { http_response_code(500); exit('save failed'); }
echo 'ok';
