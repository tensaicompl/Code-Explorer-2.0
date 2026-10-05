const express = require('express');

const app = express();

function listUsers(req, res) {
  res.json([]);
}

app.get('/users', listUsers);
app.listen(3000);
