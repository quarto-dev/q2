export function parseLocalProdPort(args, defaultPort = 8080) {
  const portIndex = args.indexOf('--port');
  if (portIndex === -1) {
    return defaultPort;
  }

  const value = args[portIndex + 1];
  if (value === undefined || !/^\d+$/.test(value)) {
    throw new Error('Expected --port to be followed by a port number');
  }

  const port = Number(value);
  if (port < 1 || port > 65535) {
    throw new Error('Port must be between 1 and 65535');
  }

  return port;
}

if (import.meta.url === `file://${process.argv[1]}`) {
  try {
    console.log(parseLocalProdPort(process.argv.slice(2)));
  } catch (error) {
    console.error(error.message);
    process.exit(1);
  }
}
