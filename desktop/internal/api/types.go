package api

type WindowBounds struct {
	Width  int `json:"width"`
	Height int `json:"height"`
	X      int `json:"x"`
	Y      int `json:"y"`
}

type Settings struct {
	APIURL             string       `json:"apiUrl"`
	Origin             string       `json:"origin"`
	NetworkAllowed     bool         `json:"networkAllowed"`
	NetworkAllowedHost string       `json:"networkAllowedHost"`
	Window             WindowBounds `json:"window"`
}

type Info struct {
	Version        string `json:"version"`
	Platform       string `json:"platform"`
	APIURL         string `json:"apiUrl"`
	Origin         string `json:"origin"`
	ResolvedOrigin string `json:"resolvedOrigin"`
	NetworkAllowed bool   `json:"networkAllowed"`
}

type Health struct {
	OK         bool   `json:"ok"`
	Status     int    `json:"status"`
	Env        string `json:"env"`
	Origin     string `json:"origin"`
	ServerTime string `json:"serverTime"`
	Message    string `json:"message"`
}

type Error struct {
	Status  int               `json:"status"`
	Code    string            `json:"code"`
	Message string            `json:"message"`
	Fields  map[string]string `json:"fields"`
}

func (e *Error) Error() string {
	if e == nil {
		return ""
	}
	return e.Message
}

type Response struct {
	OK    bool   `json:"ok"`
	Data  any    `json:"data,omitempty"`
	Error *Error `json:"error,omitempty"`
}

type SettingsResult struct {
	APIURL         string `json:"apiUrl"`
	Origin         string `json:"origin"`
	ResolvedOrigin string `json:"resolvedOrigin"`
	NetworkAllowed bool   `json:"networkAllowed"`
}

type FirewallGuide struct {
	Platform string   `json:"platform"`
	Title    string   `json:"title"`
	Intro    string   `json:"intro"`
	Steps    []string `json:"steps"`
	Note     string   `json:"note"`
}

func newError(status int, code, message string) *Error {
	return &Error{
		Status:  status,
		Code:    code,
		Message: message,
		Fields:  map[string]string{},
	}
}

func validationError(code, message string) *Error {
	return newError(0, code, message)
}
