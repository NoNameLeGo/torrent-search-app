const { execSync } = require("child_process");
const path = require("path");
const fs = require("fs");

const distDir = path.join(__dirname, "..", "dist", "portable", "BT聚合搜索");
const appDir = path.join(distDir, "resources", "app");

// Clean dist
if (fs.existsSync(distDir)) {
  fs.rmSync(distDir, { recursive: true, force: true });
}

// Create portable dir structure
fs.mkdirSync(path.join(distDir, "resources", "app"), { recursive: true });

// Copy files
const copyRecursive = (src, dest) => {
  const stat = fs.statSync(src);
  if (stat.isDirectory()) {
    fs.mkdirSync(dest, { recursive: true });
    for (const entry of fs.readdirSync(src)) {
      copyRecursive(path.join(src, entry), path.join(dest, entry));
    }
  } else {
    fs.copyFileSync(src, dest);
  }
};

const srcDir = path.join(__dirname, "..");
for (const item of fs.readdirSync(srcDir, { withFileTypes: true })) {
  if (item.name === "dist" || item.name === "node_modules" || item.name === ".cache") continue;
  const src = path.join(srcDir, item.name);
  const dest = path.join(appDir, item.name);
  if (item.isDirectory()) {
    copyRecursive(src, dest);
  } else {
    fs.copyFileSync(src, dest);
  }
}

// Install production deps in portable app
process.chdir(appDir);
execSync("npm install --omit=dev", { stdio: "inherit" });

console.log("Portable build complete:", distDir);
