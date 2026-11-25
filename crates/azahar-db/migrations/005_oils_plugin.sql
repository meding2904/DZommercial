UPDATE products
SET plugin_id = 'oils',
    attributes = json_object(
        'size', CASE WHEN trim(size) = '' THEN json('[]') ELSE json_array(size) END,
        'concerns', json(concerns),
        'ingredients', json(ingredients),
        'how_to', CASE WHEN trim(how_to) = '' THEN json('[]') ELSE json_array(how_to) END
    )
WHERE plugin_id = 'core';
