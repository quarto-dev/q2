-- Loaded from the extension's filter via require. `resolved` records what
-- resolve_path returns at module load time: per the script-dir contract
-- (GH #588) it must be the extension-root-resolved path, identical to what
-- the top-level filter script gets.
return {
  greeting = "greet-module-loaded",
  resolved = quarto.utils.resolve_path("_modules/greet.lua"),
}
