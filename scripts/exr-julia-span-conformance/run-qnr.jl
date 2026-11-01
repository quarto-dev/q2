# EXR-8 B2b: which spans does QuartoNotebookRunner evaluate in each corpus document?
# Usage (from a scratch env that Pkg.develops the QNR checkout):
#   julia --project=<env> run-qnr.jl <corpus-dir> <out.json>
# Writes {"<doc>.qmd": [{"line": n, "code": "..."}, ...]} for the spans QNR ran
# (julia language only), in document order.
import QuartoNotebookRunner as QNR
import JSON3

corpus, out = ARGS
result = Dict{String,Any}()
server = QNR.Server()
for name in sort(filter(endswith(".qmd"), readdir(corpus)))
    path = joinpath(corpus, name)
    nb = QNR.run!(
        server,
        path;
        markdown = read(path, String),
        options = Dict{String,Any}("emitUnits" => true),
        showprogress = false,
    )
    QNR.close!(server, path)
    spans = Any[]
    for c in nb.cells
        haskey(c.metadata, :quarto) || continue
        haskey(c.metadata.quarto, :inline) || continue
        for i in c.metadata.quarto.inline
            push!(spans, (; line = i.line, code = i.code))
        end
    end
    result[name] = spans
end
open(out, "w") do io
    JSON3.pretty(io, result)
end
