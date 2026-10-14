# neither deno nor node can reliably launch a process that outlives them and its
# pipes (https://github.com/denoland/deno/issues/5501), so we use an indirection
# where we start the detached julia process using julia itself
julia_bin = ARGS[1]
project = ARGS[2]
julia_file = ARGS[3]
transport_file = ARGS[4]
logfile = ARGS[5]

if length(ARGS) > 5
  error("Too many arguments")
end

env = copy(ENV)
env["JULIA_LOAD_PATH"] = "@:@stdlib" # ignore the main env
cmd = `$julia_bin --startup-file=no --project=$project $julia_file $transport_file $logfile`
cmd = setenv(cmd, env)
# Redirect the detached server's stdout/stderr to devnull so it never inherits
# the launching process's file descriptors. Otherwise the server's early output
# (Julia startup / precompile messages, printed before quartonotebookrunner.jl
# installs its own log redirect) leaks onto the caller's output streams — and
# in embedding hosts that speak a line-framed protocol on those streams, a
# single stray line is a protocol error. quartonotebookrunner.jl still writes
# the server log to `logfile` via its internal pipe, so no diagnostics are lost.
run(pipeline(detach(cmd), stdout = devnull, stderr = devnull), wait = false)
