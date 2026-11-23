package whatsapp

import (
	"context"
	"crypto/hmac"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"log/slog"
	"net/http"
	"net/http/httptest"
	"net/url"
	"os"
	"strings"
	"testing"
	"time"

	"azahar/internal/db"
	"azahar/internal/plugins"
	"azahar/internal/store"

	"github.com/go-chi/chi/v5"
)

func TestWhatsAppOrderAndWebhookLifecycle(t *testing.T) {
	dsn := os.Getenv("AZAHAR_TEST_DATABASE_URL")
	if dsn == "" {
		t.Skip("set AZAHAR_TEST_DATABASE_URL to run the WhatsApp integration test")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 60*time.Second)
	defer cancel()

	handle, err := db.Open(ctx, dsn)
	if err != nil {
		t.Fatalf("open database: %v", err)
	}
	t.Cleanup(func() { handle.Close() })
	if err := db.Migrate(ctx, handle); err != nil {
		t.Fatalf("migrate: %v", err)
	}
	instance := New()
	migrations := make([]db.PluginMigration, 0)
	for _, migration := range instance.Migrations() {
		migrations = append(migrations, db.PluginMigration{
			PluginID: pluginID, Version: migration.Version, SQL: migration.SQL,
		})
	}
	if err := db.MigratePlugins(ctx, handle, migrations); err != nil {
		t.Fatalf("plugin migrate: %v", err)
	}

	repo := store.New(handle)
	env := &plugins.Env{
		DB:        handle,
		Store:     repo,
		Logger:    slog.New(slog.NewTextHandler(io.Discard, nil)),
		SecretKey: "whatsapp-integration-secret-0123456789",
	}
	values := validSettings()
	values["enabled"] = "true"
	values["notify_cancelled"] = "false"
	sealed, err := env.EncodeSettings(values)
	if err != nil {
		t.Fatalf("encode settings: %v", err)
	}
	if err := repo.SavePluginSettings(ctx, handle, pluginID, sealed, time.Now().UTC()); err != nil {
		t.Fatalf("save settings: %v", err)
	}
	t.Cleanup(func() { handle.Exec(`DELETE FROM plugin_settings WHERE plugin_id = ?`, pluginID) })

	suffix := fmt.Sprintf("%d", time.Now().UnixNano())
	phone := localPhone(suffix)
	orderID := "ord_wa_" + suffix
	now := time.Now().UTC()
	if _, err := handle.ExecContext(ctx,
		`INSERT INTO orders (id, number, name, email, phone, address, city, notes,
		   subtotal_cents, shipping_cents, total_cents, currency, status, token_hash, created_at, updated_at)
		 VALUES (?, ?, ?, ?, ?, ?, ?, '', ?, ?, ?, 'DZD', 'pending', ?, ?, ?)`,
		orderID, "AZ-WA-"+suffix, "Amina Benali", "amina@example.com", phone,
		"22 Rue de la Paix", "Alger", 540000, 60000, 600000, "token-"+suffix,
		store.FormatTime(now), store.FormatTime(now),
	); err != nil {
		t.Fatalf("insert order: %v", err)
	}
	t.Cleanup(func() {
		handle.Exec(`DELETE FROM orders WHERE id = ?`, orderID)
		handle.Exec(`DELETE FROM whatsapp_messages WHERE order_id = ? OR phone = ?`,
			orderID, normalizedPhoneFromLocal(phone))
		handle.Exec(`DELETE FROM whatsapp_contacts WHERE phone = ?`, normalizedPhoneFromLocal(phone))
		handle.Exec(`DELETE FROM whatsapp_state`)
	})

	type sentRequest struct {
		Type     string `json:"type"`
		To       string `json:"to"`
		Template struct {
			Name     string `json:"name"`
			Language struct {
				Code string `json:"code"`
			} `json:"language"`
			Components []struct {
				Parameters []struct {
					Text string `json:"text"`
				} `json:"parameters"`
			} `json:"components"`
		} `json:"template"`
	}
	sent := make(chan sentRequest, 10)
	graph := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		var request sentRequest
		if err := json.NewDecoder(r.Body).Decode(&request); err != nil {
			t.Errorf("decode Graph request: %v", err)
		}
		sent <- request
		w.Header().Set("Content-Type", "application/json")
		_, _ = fmt.Fprintf(w, `{"messages":[{"id":"wamid.%d"}]}`, time.Now().UnixNano())
	}))
	defer graph.Close()

	instance.mu.Lock()
	instance.env = env
	instance.mu.Unlock()

	// The core enable switch stops sends even though hooks still run.
	if _, err := handle.ExecContext(ctx,
		`INSERT INTO plugin_states (plugin_id, enabled, updated_at) VALUES (?, 0, ?)
		 ON CONFLICT(plugin_id) DO UPDATE SET enabled = 0, updated_at = excluded.updated_at`,
		pluginID, store.FormatTime(time.Now().UTC())); err != nil {
		t.Fatalf("disable plugin state: %v", err)
	}
	instance.refreshConfig(ctx)
	if instance.currentConfig().Enabled {
		t.Fatal("a core-disabled plugin still reports enabled")
	}
	if _, err := handle.ExecContext(ctx,
		`UPDATE plugin_states SET enabled = 1, updated_at = ? WHERE plugin_id = ?`,
		store.FormatTime(time.Now().UTC()), pluginID); err != nil {
		t.Fatalf("enable plugin state: %v", err)
	}
	instance.refreshConfig(ctx)
	if !instance.currentConfig().Enabled || !instance.currentConfig().active() {
		t.Fatal("the plugin did not come back after the core switch was enabled")
	}
	t.Cleanup(func() { handle.Exec(`DELETE FROM plugin_states WHERE plugin_id = ?`, pluginID) })

	instance.mu.Lock()
	instance.config = parseSettings(values)
	instance.configErr = nil
	instance.baseURL = graph.URL
	instance.httpClient = graph.Client()
	instance.mu.Unlock()

	// The hook writes inside the order transaction, exactly as checkout does.
	tx, err := handle.BeginTx(ctx, nil)
	if err != nil {
		t.Fatalf("begin order transaction: %v", err)
	}
	if err := instance.OnOrderCreated(ctx, tx, plugins.OrderRef{
		ID: orderID, Number: "AZ-WA-" + suffix, Status: "pending",
	}, nil); err != nil {
		tx.Rollback()
		t.Fatalf("order created hook: %v", err)
	}
	if err := tx.Commit(); err != nil {
		t.Fatalf("commit order transaction: %v", err)
	}

	normalized := normalizedPhoneFromLocal(phone)
	var kind, storedPhone, status, body, params string
	var total int64
	if err := handle.QueryRowContext(ctx,
		`SELECT kind, phone, status, body, template_params FROM whatsapp_messages WHERE order_id = ?`,
		orderID,
	).Scan(&kind, &storedPhone, &status, &body, &params); err != nil {
		t.Fatalf("read queued confirmation: %v", err)
	}
	if kind != "template" || storedPhone != normalized || status != "queued" {
		t.Fatalf("queued confirmation = kind %q phone %q status %q", kind, storedPhone, status)
	}
	var decodedParams []string
	if err := json.Unmarshal([]byte(params), &decodedParams); err != nil {
		t.Fatalf("decode template params: %v", err)
	}
	if len(decodedParams) != 3 || decodedParams[0] != "Amina" || decodedParams[1] != "AZ-WA-"+suffix ||
		decodedParams[2] != "6,000 DZD" {
		t.Fatalf("template params = %#v in body %q", decodedParams, body)
	}

	// The dispatcher claims the row and sends the approved template.
	instance.dispatch(ctx)
	select {
	case request := <-sent:
		if request.Type != "template" || request.Template.Name != "order_confirmation" ||
			request.Template.Language.Code != "en" || request.To != strings.TrimPrefix(normalized, "+") {
			t.Fatalf("Graph request = %#v", request)
		}
		if len(request.Template.Components) != 1 ||
			len(request.Template.Components[0].Parameters) != 3 ||
			request.Template.Components[0].Parameters[2].Text != "6,000 DZD" {
			t.Fatalf("Graph parameters = %#v", request.Template.Components)
		}
	case <-time.After(5 * time.Second):
		t.Fatal("the dispatcher made no Graph request")
	}
	var metaID string
	if err := handle.QueryRowContext(ctx,
		`SELECT status, meta_message_id FROM whatsapp_messages WHERE order_id = ?`,
		orderID).Scan(&status, &metaID); err != nil {
		t.Fatalf("read sent confirmation: %v", err)
	}
	if status != "sent" || metaID == "" {
		t.Fatalf("confirmation after dispatch = status %q id %q", status, metaID)
	}

	// A shipped status queues a second message with the status template.
	tx, err = handle.BeginTx(ctx, nil)
	if err != nil {
		t.Fatalf("begin status transaction: %v", err)
	}
	if err := instance.OnOrderStatusChanged(ctx, tx, orderID, "packed", "shipped"); err != nil {
		tx.Rollback()
		t.Fatalf("status hook: %v", err)
	}
	if err := tx.Commit(); err != nil {
		t.Fatalf("commit status transaction: %v", err)
	}
	if err := handle.QueryRowContext(ctx,
		`SELECT COUNT(*) FROM whatsapp_messages WHERE order_id = ?`, orderID).Scan(&total); err != nil {
		t.Fatalf("count status message: %v", err)
	}
	if total != 2 {
		t.Fatalf("status message count = %d, want 2", total)
	}

	// A signed webhook marks the confirmation delivered and records STOP.
	digits := strings.TrimPrefix(normalized, "+")
	event := fmt.Sprintf(`{"object":"whatsapp_business_account","entry":[{"id":"1","changes":[{"field":"messages","value":{`+
		`"messaging_product":"whatsapp","metadata":{"display_phone_number":"213","phone_number_id":"123456789012345"},`+
		`"contacts":[{"profile":{"name":"Amina Benali"},"wa_id":"%s"}],`+
		`"statuses":[{"id":"%s","status":"delivered","timestamp":"%d","recipient_id":"%s"}],`+
		`"messages":[{"from":"%s","id":"wamid.in.%s","timestamp":"%d","type":"text","text":{"body":"STOP"}}]}}]}]}`,
		digits, metaID, now.Unix(), digits, digits, suffix, now.Unix())
	postWebhook(t, instance, env, values["app_secret"], event, http.StatusOK)

	if err := handle.QueryRowContext(ctx,
		`SELECT status FROM whatsapp_messages WHERE order_id = ? AND meta_message_id = ?`,
		orderID, metaID).Scan(&status); err != nil {
		t.Fatalf("read delivered status: %v", err)
	}
	if status != "delivered" {
		t.Fatalf("confirmation status = %q, want delivered", status)
	}
	var inbound, optedIn int
	if err := handle.QueryRowContext(ctx,
		`SELECT COUNT(*) FROM whatsapp_messages WHERE direction = 'in' AND meta_message_id = ?`,
		"wamid.in."+suffix).Scan(&inbound); err != nil {
		t.Fatalf("count inbound: %v", err)
	}
	if inbound != 1 {
		t.Fatalf("inbound rows = %d, want 1", inbound)
	}
	if err := handle.QueryRowContext(ctx,
		`SELECT opted_in FROM whatsapp_contacts WHERE phone = ?`, normalized).Scan(&optedIn); err != nil {
		t.Fatalf("read consent: %v", err)
	}
	if optedIn != 0 {
		t.Fatalf("opted_in = %d after STOP, want 0", optedIn)
	}

	// Duplicate delivery of the same webhook body is harmless.
	postWebhook(t, instance, env, values["app_secret"], event, http.StatusOK)
	var inboundAfter int
	_ = handle.QueryRowContext(ctx,
		`SELECT COUNT(*) FROM whatsapp_messages WHERE direction = 'in' AND meta_message_id = ?`,
		"wamid.in."+suffix).Scan(&inboundAfter)
	if inboundAfter != 1 {
		t.Fatalf("duplicate webhook created %d inbound rows", inboundAfter)
	}

	// STOP also cancels the status update that was already queued.
	instance.dispatch(ctx)
	select {
	case request := <-sent:
		t.Fatalf("a queued message was sent after STOP: %#v", request)
	case <-time.After(200 * time.Millisecond):
	}
	var cancelled int
	if err := handle.QueryRowContext(ctx,
		`SELECT COUNT(*) FROM whatsapp_messages WHERE order_id = ? AND status = 'cancelled'`,
		orderID).Scan(&cancelled); err != nil {
		t.Fatalf("count cancelled message: %v", err)
	}
	if cancelled != 1 {
		t.Fatalf("queued message after STOP was not cancelled: %d", cancelled)
	}

	// A cancelled status is disabled, and an opted-out contact gets no message.
	tx, err = handle.BeginTx(ctx, nil)
	if err != nil {
		t.Fatalf("begin cancel transaction: %v", err)
	}
	if err := instance.OnOrderStatusChanged(ctx, tx, orderID, "pending", "cancelled"); err != nil {
		tx.Rollback()
		t.Fatalf("cancel hook: %v", err)
	}
	if err := tx.Commit(); err != nil {
		t.Fatalf("commit cancel transaction: %v", err)
	}
	tx, err = handle.BeginTx(ctx, nil)
	if err != nil {
		t.Fatalf("begin delivered transaction: %v", err)
	}
	if err := instance.OnOrderStatusChanged(ctx, tx, orderID, "shipped", "delivered"); err != nil {
		tx.Rollback()
		t.Fatalf("delivered hook: %v", err)
	}
	if err := tx.Commit(); err != nil {
		t.Fatalf("commit delivered transaction: %v", err)
	}
	var messages int
	_ = handle.QueryRowContext(ctx,
		`SELECT COUNT(*) FROM whatsapp_messages WHERE order_id = ?`, orderID).Scan(&messages)
	if messages != 2 {
		t.Fatalf("opted-out or cancelled order queued a message: %d rows", messages)
	}

	// START re-opens consent. The next delivery update is queued.
	startEvent := fmt.Sprintf(`{"object":"whatsapp_business_account","entry":[{"id":"1","changes":[{"field":"messages","value":{`+
		`"messaging_product":"whatsapp","contacts":[{"profile":{"name":"Amina Benali"},"wa_id":"%s"}],`+
		`"messages":[{"from":"%s","id":"wamid.start.%s","timestamp":"%d","type":"text","text":{"body":"START"}}]}}]}]}`,
		digits, digits, suffix, now.Unix()+1)
	postWebhook(t, instance, env, values["app_secret"], startEvent, http.StatusOK)
	if err := handle.QueryRowContext(ctx,
		`SELECT opted_in FROM whatsapp_contacts WHERE phone = ?`, normalized).Scan(&optedIn); err != nil {
		t.Fatalf("read restored consent: %v", err)
	}
	if optedIn != 1 {
		t.Fatalf("opted_in = %d after START, want 1", optedIn)
	}

	tx, err = handle.BeginTx(ctx, nil)
	if err != nil {
		t.Fatalf("begin delivered transaction: %v", err)
	}
	if err := instance.OnOrderStatusChanged(ctx, tx, orderID, "shipped", "delivered"); err != nil {
		tx.Rollback()
		t.Fatalf("delivered hook: %v", err)
	}
	if err := tx.Commit(); err != nil {
		t.Fatalf("commit delivered transaction: %v", err)
	}
	_ = handle.QueryRowContext(ctx,
		`SELECT COUNT(*) FROM whatsapp_messages WHERE order_id = ?`, orderID).Scan(&messages)
	if messages != 3 {
		t.Fatalf("restored consent did not queue the delivery update: %d rows", messages)
	}

	// A read receipt cannot be downgraded by a late delivered receipt.
	readEvent := fmt.Sprintf(`{"object":"whatsapp_business_account","entry":[{"id":"1","changes":[{"field":"messages","value":{`+
		`"messaging_product":"whatsapp","statuses":[{"id":"%s","status":"read","timestamp":"%d"}]}}]}]}`,
		metaID, now.Unix()+2)
	postWebhook(t, instance, env, values["app_secret"], readEvent, http.StatusOK)
	lateEvent := fmt.Sprintf(`{"object":"whatsapp_business_account","entry":[{"id":"1","changes":[{"field":"messages","value":{`+
		`"messaging_product":"whatsapp","statuses":[{"id":"%s","status":"delivered","timestamp":"%d"}]}}]}]}`,
		metaID, now.Unix()+3)
	postWebhook(t, instance, env, values["app_secret"], lateEvent, http.StatusOK)
	if err := handle.QueryRowContext(ctx,
		`SELECT status FROM whatsapp_messages WHERE meta_message_id = ?`, metaID).Scan(&status); err != nil {
		t.Fatalf("read receipt status: %v", err)
	}
	if status != "read" {
		t.Fatalf("status after late delivered receipt = %q, want read", status)
	}

	// Bad signatures are refused before any database work.
	postWebhook(t, instance, env, "wrong-secret", event, http.StatusForbidden)

	// The verification handshake echoes only the configured challenge.
	verify := httptest.NewRequest(http.MethodGet,
		"/api/plugins/whatsapp/webhook?hub.mode=subscribe&hub.verify_token="+values["verify_token"]+"&hub.challenge=abc123", nil)
	recorder := httptest.NewRecorder()
	instance.handleWebhookVerify(&plugins.Context{W: recorder, R: verify, Env: env, PluginID: pluginID})
	if recorder.Code != http.StatusOK || recorder.Body.String() != "abc123" {
		t.Fatalf("verification = %d %q", recorder.Code, recorder.Body.String())
	}
	badVerify := httptest.NewRequest(http.MethodGet,
		"/api/plugins/whatsapp/webhook?hub.mode=subscribe&hub.verify_token=wrong&hub.challenge=abc123", nil)
	recorder = httptest.NewRecorder()
	instance.handleWebhookVerify(&plugins.Context{W: recorder, R: badVerify, Env: env, PluginID: pluginID})
	if recorder.Code != http.StatusForbidden {
		t.Fatalf("wrong verify token status = %d, want 403", recorder.Code)
	}

	// The admin surface reports the connection, the log and the consent state.
	context, recorder := adminCall(env, http.MethodGet, "/api/plugins/whatsapp/admin/status", "", nil)
	instance.handleStatus(context)
	if recorder.Code != http.StatusOK {
		t.Fatalf("status screen = %d: %s", recorder.Code, recorder.Body.String())
	}
	var statusScreen statusDTO
	if err := json.Unmarshal(recorder.Body.Bytes(), &statusScreen); err != nil {
		t.Fatalf("decode status screen: %v", err)
	}
	if !statusScreen.Configured || !statusScreen.Enabled || statusScreen.PhoneNumberID == "" {
		t.Fatalf("status screen = %#v", statusScreen)
	}
	if statusScreen.Counts["read"] < 1 || statusScreen.Contacts < 1 {
		t.Fatalf("status counts = %#v", statusScreen.Counts)
	}

	context, recorder = adminCall(env, http.MethodGet, "/api/plugins/whatsapp/admin/messages?limit=10", "", nil)
	instance.handleMessages(context)
	if recorder.Code != http.StatusOK {
		t.Fatalf("message log = %d: %s", recorder.Code, recorder.Body.String())
	}
	var logResponse struct {
		Messages []messageDTO `json:"messages"`
	}
	if err := json.Unmarshal(recorder.Body.Bytes(), &logResponse); err != nil {
		t.Fatalf("decode message log: %v", err)
	}
	if len(logResponse.Messages) == 0 {
		t.Fatalf("message log = %#v", logResponse.Messages)
	}
	foundOrder := false
	for _, message := range logResponse.Messages {
		if message.OrderNumber != "" {
			foundOrder = true
			break
		}
	}
	if !foundOrder {
		t.Fatalf("message log carried no linked order: %#v", logResponse.Messages)
	}

	context, recorder = adminCall(env, http.MethodGet, "/api/plugins/whatsapp/admin/contacts?limit=10", "", nil)
	instance.handleContacts(context)
	if recorder.Code != http.StatusOK {
		t.Fatalf("contacts = %d: %s", recorder.Code, recorder.Body.String())
	}
	var contactsResponse struct {
		Contacts []contactDTO `json:"contacts"`
	}
	if err := json.Unmarshal(recorder.Body.Bytes(), &contactsResponse); err != nil {
		t.Fatalf("decode contacts: %v", err)
	}
	if len(contactsResponse.Contacts) == 0 || !contactsResponse.Contacts[0].OptedIn {
		t.Fatalf("contacts = %#v", contactsResponse.Contacts)
	}

	// Pausing consent blocks a template before it reaches the queue.
	context, recorder = adminCall(env, http.MethodPatch,
		"/api/plugins/whatsapp/admin/contacts/"+url.PathEscape(normalized),
		`{"opted_in":false}`, map[string]string{"phone": normalized})
	instance.handleUpdateContact(context)
	if recorder.Code != http.StatusOK {
		t.Fatalf("pause contact = %d: %s", recorder.Code, recorder.Body.String())
	}
	blocked := fmt.Sprintf(`{"phone":%q,"kind":"template","template_name":"order_update","template_language":"en","template_params":["Amina"]}`, normalized)
	context, recorder = adminCall(env, http.MethodPost, "/api/plugins/whatsapp/admin/messages", blocked, nil)
	instance.handleSendMessage(context)
	if recorder.Code != http.StatusConflict || !strings.Contains(recorder.Body.String(), "contact_opted_out") {
		t.Fatalf("paused send = %d: %s", recorder.Code, recorder.Body.String())
	}

	context, recorder = adminCall(env, http.MethodPatch,
		"/api/plugins/whatsapp/admin/contacts/"+url.PathEscape(normalized),
		`{"opted_in":true}`, map[string]string{"phone": normalized})
	instance.handleUpdateContact(context)
	if recorder.Code != http.StatusOK {
		t.Fatalf("resume contact = %d: %s", recorder.Code, recorder.Body.String())
	}

	// A free-form reply outside the 24-hour window is refused.
	unknown := "+213" + strings.Repeat("9", 9)
	textOutside := fmt.Sprintf(`{"phone":%q,"kind":"text","text":"Hello"}`, unknown)
	context, recorder = adminCall(env, http.MethodPost, "/api/plugins/whatsapp/admin/messages", textOutside, nil)
	instance.handleSendMessage(context)
	if recorder.Code != http.StatusConflict || !strings.Contains(recorder.Body.String(), "reply_window_closed") {
		t.Fatalf("closed-window send = %d: %s", recorder.Code, recorder.Body.String())
	}

	// A template send queues a new row and returns it.
	sendBody := fmt.Sprintf(`{"phone":%q,"kind":"template","template_name":"order_update","template_language":"en","template_params":["Amina","AZ-WA-%s","delivered","6,000 DZD"],"order_id":%q}`,
		normalized, suffix, orderID)
	context, recorder = adminCall(env, http.MethodPost, "/api/plugins/whatsapp/admin/messages", sendBody, nil)
	instance.handleSendMessage(context)
	if recorder.Code != http.StatusCreated {
		t.Fatalf("template send = %d: %s", recorder.Code, recorder.Body.String())
	}
	var sendResponse struct {
		Message messageDTO `json:"message"`
	}
	if err := json.Unmarshal(recorder.Body.Bytes(), &sendResponse); err != nil {
		t.Fatalf("decode sent message: %v", err)
	}
	if sendResponse.Message.Status != "queued" || sendResponse.Message.OrderNumber == "" {
		t.Fatalf("queued message = %#v", sendResponse.Message)
	}

	// A failed row is retryable and resets its attempt count.
	if _, err := handle.ExecContext(ctx,
		`UPDATE whatsapp_messages SET status = 'failed', attempts = 5, error = 'Meta rejected it' WHERE id = ?`,
		sendResponse.Message.ID); err != nil {
		t.Fatalf("mark failed: %v", err)
	}
	context, recorder = adminCall(env, http.MethodPost,
		"/api/plugins/whatsapp/admin/messages/"+url.PathEscape(sendResponse.Message.ID)+"/retry", "",
		map[string]string{"id": sendResponse.Message.ID})
	instance.handleRetryMessage(context)
	if recorder.Code != http.StatusOK {
		t.Fatalf("retry = %d: %s", recorder.Code, recorder.Body.String())
	}
	var retryResponse struct {
		Message messageDTO `json:"message"`
	}
	if err := json.Unmarshal(recorder.Body.Bytes(), &retryResponse); err != nil {
		t.Fatalf("decode retry: %v", err)
	}
	if retryResponse.Message.Status != "queued" || retryResponse.Message.Attempts != 0 {
		t.Fatalf("retried message = %#v", retryResponse.Message)
	}
}

func adminCall(env *plugins.Env, method, path, body string, params map[string]string) (*plugins.Context, *httptest.ResponseRecorder) {
	request := httptest.NewRequest(method, path, strings.NewReader(body))
	if body != "" {
		request.Header.Set("Content-Type", "application/json")
	}
	routeContext := chi.NewRouteContext()
	for key, value := range params {
		routeContext.URLParams.Add(key, value)
	}
	request = request.WithContext(context.WithValue(request.Context(), chi.RouteCtxKey, routeContext))
	recorder := httptest.NewRecorder()
	return &plugins.Context{
		W: recorder, R: request, Env: env, PluginID: pluginID, UserRole: "admin",
	}, recorder
}

func postWebhook(t *testing.T, instance *Plugin, env *plugins.Env, secret, body string, want int) {
	t.Helper()
	request := httptest.NewRequest(http.MethodPost, "/api/plugins/whatsapp/webhook", strings.NewReader(body))
	mac := hmac.New(sha256.New, []byte(secret))
	mac.Write([]byte(body))
	request.Header.Set("X-Hub-Signature-256", "sha256="+hex.EncodeToString(mac.Sum(nil)))
	recorder := httptest.NewRecorder()
	instance.handleWebhookEvent(&plugins.Context{W: recorder, R: request, Env: env, PluginID: pluginID})
	if recorder.Code != want {
		t.Fatalf("webhook status = %d, want %d (%s)", recorder.Code, want, recorder.Body.String())
	}
}

func localPhone(suffix string) string {
	digits := strings.ReplaceAll(suffix, "-", "")
	if len(digits) > 8 {
		digits = digits[len(digits)-8:]
	}
	return "05" + digits
}

func normalizedPhoneFromLocal(local string) string {
	value, _ := normalizePhone(local, "213")
	return value
}
