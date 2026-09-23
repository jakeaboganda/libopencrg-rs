/*
 * Evaluates queries with the ASAM OpenCRG C-API and prints full-precision results.
 *
 * Usage: crg-oracle FILE < QUERIES
 *
 * Loads FILE the way the C-API demos do (read, check, apply modifiers, create a contact
 * point), except that a failed crgCheck is reported on stderr instead of stopping. Then
 * reads one command per line from stdin and writes one result line to stdout:
 *
 *   range          -> umin umax vmin vmax
 *   z U V          -> z
 *   xy U V         -> x y
 *   pk U V         -> phi curvature
 *   uv X Y         -> u v     (uses the contact point's search history)
 *   reset          -> ok      (new contact point: empty history, file options)
 *   opti ID VALUE  -> ok      (crgContactPointOptionSetInt)
 *   optd ID VALUE  -> ok      (crgContactPointOptionSetDouble)
 *
 * A failed evaluation prints "none". Numbers use %.17g, which round-trips doubles.
 * Blank lines and lines starting with '#' are echoed unchanged.
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "crgBaseLib.h"

static void print2( int ok, const double* a, const double* b )
{
    if ( ok )
        printf( "%.17g %.17g\n", *a, *b );
    else
        printf( "none\n" );
}

int main( int argc, char** argv )
{
    char   line[256];
    char   cmd[16];
    double a, b, r0, r1, r2, r3;
    int    id, dataSetId, cpId;

    if ( argc != 2 )
    {
        fprintf( stderr, "usage: %s FILE < QUERIES\n", argv[0] );
        return 2;
    }

    crgMsgSetLevel( getenv( "CRG_ORACLE_VERBOSE" ) ? dCrgMsgLevelWarn : dCrgMsgLevelFatal );

    if ( ( dataSetId = crgLoaderReadFile( argv[1] ) ) <= 0 )
    {
        fprintf( stderr, "cannot load %s\n", argv[1] );
        return 1;
    }
    if ( !crgCheck( dataSetId ) )
        fprintf( stderr, "crgCheck failed for %s; evaluating anyway\n", argv[1] );
    crgDataSetModifiersApply( dataSetId );

    if ( ( cpId = crgContactPointCreate( dataSetId ) ) < 0 )
        return 1;

    while ( fgets( line, sizeof( line ), stdin ) )
    {
        if ( line[0] == '\n' || line[0] == '#' )
        {
            fputs( line, stdout );
            continue;
        }
        if ( sscanf( line, "%15s", cmd ) != 1 )
            continue;

        if ( !strcmp( cmd, "range" ) )
        {
            crgDataSetGetURange( dataSetId, &r0, &r1 );
            crgDataSetGetVRange( dataSetId, &r2, &r3 );
            printf( "%.17g %.17g %.17g %.17g\n", r0, r1, r2, r3 );
        }
        else if ( !strcmp( cmd, "reset" ) )
        {
            crgContactPointDelete( cpId );
            cpId = crgContactPointCreate( dataSetId );
            printf( "ok\n" );
        }
        else if ( !strcmp( cmd, "opti" ) && sscanf( line, "%*s %d %lf", &id, &a ) == 2 )
            printf( crgContactPointOptionSetInt( cpId, id, ( int ) a ) ? "ok\n" : "none\n" );
        else if ( !strcmp( cmd, "optd" ) && sscanf( line, "%*s %d %lf", &id, &a ) == 2 )
            printf( crgContactPointOptionSetDouble( cpId, id, a ) ? "ok\n" : "none\n" );
        else if ( sscanf( line, "%*s %lf %lf", &a, &b ) != 2 )
        {
            fprintf( stderr, "bad query: %s", line );
            return 2;
        }
        else if ( !strcmp( cmd, "z" ) )
        {
            if ( crgEvaluv2z( cpId, a, b, &r0 ) )
                printf( "%.17g\n", r0 );
            else
                printf( "none\n" );
        }
        else if ( !strcmp( cmd, "xy" ) )
            print2( crgEvaluv2xy( cpId, a, b, &r0, &r1 ), &r0, &r1 );
        else if ( !strcmp( cmd, "pk" ) )
            print2( crgEvaluv2pk( cpId, a, b, &r0, &r1 ), &r0, &r1 );
        else if ( !strcmp( cmd, "uv" ) )
            print2( crgEvalxy2uv( cpId, a, b, &r0, &r1 ), &r0, &r1 );
        else
        {
            fprintf( stderr, "unknown command: %s", line );
            return 2;
        }
    }
    return 0;
}
