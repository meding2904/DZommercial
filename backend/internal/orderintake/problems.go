package orderintake

// Problem is one refusal the caller turns into its own problem response. The
// status, code, message and fields are the caller's to render; this package
// only decides which refusal an order earns.
type Problem struct {
	Status  int
	Code    string
	Message string
	Fields  map[string]string
}

func (p *Problem) Error() string {
	return p.Message
}

func refuse(status int, code, message string) *Problem {
	return &Problem{Status: status, Code: code, Message: message}
}

func refuseFields(status int, code, message string, fields map[string]string) *Problem {
	return &Problem{Status: status, Code: code, Message: message, Fields: fields}
}
