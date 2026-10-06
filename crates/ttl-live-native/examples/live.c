/* ./live @creator /path/to/trusted-bundle.js ["sessionid=...; ttwid=..."]
 * The application reads explicit local source; the library never downloads it.
 */
#include <ttl/ttl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
static void on_event(void *userdata, const ttl_event_t *event) {
    (void)userdata;
    printf("%.*s: %zu fields, %zu bytes\n", (int)ttl_event_method_len(event),ttl_event_method(event),
        ttl_object_field_count(ttl_event_root(event)),ttl_event_raw_size(event));
    char *json = ttl_event_to_json(event,TTL_JSON_FULL);
    if (json != NULL) { puts(json);ttl_string_free(json); }
}
static void on_error(void *userdata,const ttl_error_info_t *error) {
    (void)userdata;
    fprintf(stderr,"LIVE error %d: %.*s\n",(int)error->code,(int)error->message_len,error->message);
}
int main(int argc,char **argv) {
    if (argc < 3) { fprintf(stderr,"usage: %s @creator trusted-bundle.js [cookies]\n",argv[0]);return 2; }
    FILE *input = fopen(argv[2],"rb");if (input == NULL) { perror("bundle");return 2; }
    if (fseek(input,0,SEEK_END) != 0) { fclose(input);return 2; }
    long size = ftell(input);if (size <= 0 || size > 8*1024*1024) { fclose(input);return 2; }
    rewind(input);
    char *bundle = (char *)malloc((size_t)size);
    if (bundle == NULL) { fclose(input);return 2; }
    if (fread(bundle,1,(size_t)size,input) != (size_t)size) { free(bundle);fclose(input);return 2; }
    fclose(input);
    ttl_live_options_t options;memset(&options,0,sizeof(options));
    options.struct_size = (uint32_t)sizeof(options);
    options.unique_id = argv[1];options.unique_id_len = strlen(argv[1]);
    options.bundle = bundle;options.bundle_len = (size_t)size;
    options.reconnect = true;options.max_reconnect_attempts = 5;
    if (argc > 3) { options.cookie_header = argv[3];options.cookie_header_len = strlen(argv[3]); }
    ttl_live_client_t *client = ttl_live_client_create(&options);free(bundle);
    if (client == NULL) { fprintf(stderr,"create: %s\n",ttl_last_error());return 1; }
    ttl_live_client_set_event_callback(client,on_event,NULL);
    ttl_live_client_set_error_callback(client,on_error,NULL);
    if (ttl_live_client_connect(client) != TTL_OK) { fprintf(stderr,"connect: %s\n",ttl_last_error());ttl_live_client_destroy(client);return 1; }
    puts("Press Enter to disconnect.");(void)getchar();
    ttl_live_client_destroy(client);return 0;
}
