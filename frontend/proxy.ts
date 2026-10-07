import { NextResponse, type NextRequest } from 'next/server'

// Keep the unfinished exchange routes off the public site until launch.
export function proxy(request: NextRequest) {
  if (request.nextUrl.pathname === '/') {
    return NextResponse.next()
  }

  return NextResponse.redirect(new URL('/', request.url), 307)
}

export const config = {
  matcher: [
    '/((?!api(?:/|$)|_next(?:/|$)|video(?:/|$)|images(?:/|$)|icon\\.svg$|opengraph-image(?:/|$)|favicon\\.ico$|robots\\.txt$|sitemap\\.xml$).*)',
  ],
}
