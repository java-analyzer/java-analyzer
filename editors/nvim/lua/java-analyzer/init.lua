local M = {}

M.defaults = {
  cmd = { "java-analyzer" },
  filetypes = { "java" },
  root_markers = {
    ".java-analyzer.toml",
    "pom.xml",
    "build.gradle",
    "build.gradle.kts",
    "settings.gradle",
    "settings.gradle.kts",
    "build.xml",
    "ivy.xml",
    "mvnw",
    "gradlew",
    ".jj",
    ".git",
  },
}

--- Builds a `vim.lsp.Config`-shaped table, merging `opts` over the defaults.
---@param opts? table
---@return table
function M.config(opts)
  local cfg = vim.tbl_deep_extend("force", {}, M.defaults, opts or {})
  return {
    cmd = cfg.cmd,
    filetypes = cfg.filetypes,
    root_markers = cfg.root_markers,
  }
end

--- Registers and enables the server.
--- Uses `vim.lsp.config`/`vim.lsp.enable` on Neovim 0.11+, and falls back to a
--- FileType autocmd with `vim.lsp.start` on Neovim 0.8-0.10.
---@param opts? table
function M.setup(opts)
  local cfg = M.config(opts)

  if vim.lsp.config and vim.lsp.enable then
    vim.lsp.config("java_analyzer", cfg)
    vim.lsp.enable("java_analyzer")
    return
  end

  vim.api.nvim_create_autocmd("FileType", {
    group = vim.api.nvim_create_augroup("JavaAnalyzer", { clear = true }),
    pattern = cfg.filetypes,
    callback = function(args)
      local root = vim.fs.root and vim.fs.root(args.buf, cfg.root_markers)
        or vim.fs.dirname(vim.fs.find(cfg.root_markers, {
          path = vim.api.nvim_buf_get_name(args.buf),
          upward = true,
        })[1])
      vim.lsp.start({
        name = "java_analyzer",
        cmd = cfg.cmd,
        root_dir = root,
      }, { bufnr = args.buf })
    end,
  })
end

return M
