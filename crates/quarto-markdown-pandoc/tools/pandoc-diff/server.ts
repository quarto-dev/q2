import express from 'express';
import cors from 'cors';
import { exec } from 'child_process';
import { promisify } from 'util';
import path from 'path';
import { fileURLToPath } from 'url';
import tmp from 'tmp';
import fs from 'fs/promises';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

const execAsync = promisify(exec);
const app = express();
const PORT = 3001;

app.use(cors());
app.use(express.json());

interface CompareResult {
  pandoc: any;
  qmd: any;
  pandocError?: string;
  qmdError?: string;
}

app.post('/compare', async (req, res) => {
  const { markdown } = req.body;

  if (!markdown) {
    return res.status(400).json({ error: 'No markdown provided' });
  }

  const result: CompareResult = {
    pandoc: null,
    qmd: null,
  };

  // Create a temporary file
  const tmpFile = tmp.fileSync({ postfix: '.md' });

  try {
    // Write markdown to temp file
    await fs.writeFile(tmpFile.name, markdown, 'utf-8');

    // Run pandoc
    try {
      const { stdout } = await execAsync(`pandoc -t json "${tmpFile.name}"`, {
        maxBuffer: 10 * 1024 * 1024, // 10MB buffer
      });
      result.pandoc = JSON.parse(stdout);
    } catch (error: any) {
      result.pandocError = error.message;
    }

    // Run quarto-markdown-pandoc
    try {
      const { stdout, stderr } = await execAsync(`cargo run --bin quarto-markdown-pandoc -- -i "${tmpFile.name}" -t json`, {
        cwd: path.resolve(__dirname, '../../../..'),
        maxBuffer: 10 * 1024 * 1024, // 10MB buffer
      });
      result.qmd = JSON.parse(stdout);
    } catch (error: any) {
      result.qmdError = `${error.message}\n\nStderr: ${error.stderr || 'none'}`;
    }
  } finally {
    // Clean up temp file
    tmpFile.removeCallback();
  }

  res.json(result);
});

app.listen(PORT, () => {
  console.log(`Server running on http://localhost:${PORT}`);
});
