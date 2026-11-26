type Context = { params: Promise<{ path: string[] }> }

async function forward(request: Request, { params }: Context) {
  const target = process.env.ENDEMIC_API_URL
  if (!target) return new Response(null, { status: 404 })
  const { path } = await params
  const response = await fetch(`${target}/${path.map(encodeURIComponent).join('/')}${new URL(request.url).search}`, {
    method: request.method,
    headers: { Authorization: `Bearer ${process.env.ENDEMIC_API_TOKEN}`, 'Content-Type': request.headers.get('content-type') ?? 'application/json' },
    body: request.method === 'GET' ? undefined : await request.arrayBuffer(),
    cache: 'no-store',
  })
  return new Response(response.body, {
    status: response.status,
    headers: { 'Content-Type': response.headers.get('content-type') ?? 'application/json', 'Cache-Control': 'no-store' },
  })
}

export { forward as DELETE, forward as GET, forward as POST }
