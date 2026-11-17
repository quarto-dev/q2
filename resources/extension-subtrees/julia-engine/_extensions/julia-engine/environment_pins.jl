using TOML

# `Pkg.is_manifest_current` compares a hash of Project.toml's deps and compat, and ignores the
# `[sources]` table. So moving the pinned `rev` of QuartoNotebookRunner in Project.toml leaves a
# cached Manifest.toml "current", and `ensure_environment.jl` would only re-instantiate the old
# revision. This compares the pinned `rev` of each `[sources]` entry with the `repo-rev` the
# manifest resolved for the same package, so a pin bump forces a fresh resolve.
#
# A project with no `[sources]` pins has nothing to compare and always matches.
function sources_pins_match(project_file::AbstractString, manifest_file::AbstractString)::Bool
  sources = get(TOML.parsefile(project_file), "sources", Dict{String,Any}())
  pinned = Dict(
    name => spec["rev"] for (name, spec) in sources if spec isa AbstractDict && haskey(spec, "rev")
  )
  isempty(pinned) && return true
  deps = get(TOML.parsefile(manifest_file), "deps", Dict{String,Any}())
  for (name, rev) in pinned
    entries = get(deps, name, [])
    (entries isa AbstractVector && !isempty(entries)) || return false
    get(first(entries), "repo-rev", nothing) == rev || return false
  end
  return true
end
