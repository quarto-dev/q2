# it appears that deno cannot launch detached processes https://github.com/denoland/deno/issues/5501
# so we use an indirection where we start the detached julia process using julia itself
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
# the engine-host's stdout fd. Otherwise the QNR process's early output (Julia
# startup / precompile banners, before quartonotebookrunner.jl installs its own
# redirect) can land on the Deno engine-host's protocol channel, where a single
# non-JSON line is treated as a wire-framing error and kills the whole host,
# discarding every in-flight capture (Bug C, task-p0-report.md §Bug C).
# quartonotebookrunner.jl still writes its own log to `logfile` via its internal
# pipe, so this loses no server-log diagnostics.
run(pipeline(detach(cmd), stdout = devnull, stderr = devnull), wait = false)
